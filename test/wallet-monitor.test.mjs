import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  buildPumpPortalLocalTradeRequest,
  buildPumpPortalLocalTransaction,
  buildPumpPortalLocalTransactions,
  copySolAmountForSubscriber,
  formatCopyCandidateMessage,
  isCopyCandidateTrade
} from "../dist/copy-trade.js";
import { createTelegramCommandPoller } from "../dist/commands.js";
import { buildHeliusWebhookPayload, createHeliusWebhookServer, syncHeliusWebhook } from "../dist/helius.js";
import { heliusEventMentionsWatchedWallet, normalizeHeliusSwapData } from "../dist/helius-swaps.js";
import { createSubscriberStore } from "../dist/subscribers.js";
import {
  buildWalletTradeReplyMarkup,
  formatWalletTradeMessage,
  getWalletTradeEventId,
  isValidSolanaAddress
} from "../dist/wallet-monitor.js";

const wallet = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
const otherWallet = "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV";
const mint = "DezXAZ8z7PnrnRJjz3n26VsZdrmRwNny4nBj9JkzjW7B";
const config = {
  pumpFunBaseUrl: "https://pump.fun",
  solscanBaseUrl: "https://solscan.io",
  copyDefaultSolAmount: 0.01,
  pumpPortalLocalTradeUrl: "https://pumpportal.test/api/trade-local",
  pumpPortalLocalSlippage: 10,
  pumpPortalLocalPriorityFee: 0.00005,
  pumpPortalLocalPool: "auto"
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
    assert.equal(await store.setCopyWallet("chat-1", otherWallet), true);
    assert.equal(await store.setCopyWallet("chat-1", wallet), true);
    assert.equal(await store.setCopySolAmount("chat-1", 0.025), true);
    assert.deepEqual(
      store.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
      [[wallet, "Alpha <Wallet>"]]
    );
    assert.equal(store.get("chat-1").copyWallet, otherWallet);
    assert.deepEqual(store.get("chat-1").copyWallets, [otherWallet, wallet]);
    assert.deepEqual(store.listCopyWallets("chat-1"), [otherWallet, wallet]);
    assert.equal(store.get("chat-1").copySolAmount, 0.025);
    assert.match(store.get("chat-1").copySettingsUpdatedAt, /^\d{4}-/);

    const reloaded = createSubscriberStore({ path });
    await reloaded.init();
    assert.deepEqual(
      reloaded.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
      [[wallet, "Alpha <Wallet>"]]
    );
    assert.equal(reloaded.get("chat-1").copyWallet, otherWallet);
    assert.deepEqual(reloaded.get("chat-1").copyWallets, [otherWallet, wallet]);
    assert.equal(reloaded.get("chat-1").copySolAmount, 0.025);
    assert.equal(await reloaded.removeCopyWallet("chat-1", wallet), true);
    assert.deepEqual(reloaded.listCopyWallets("chat-1"), [otherWallet]);

    assert.equal(await reloaded.unwatchWallet("chat-1", wallet), true);
    assert.deepEqual(reloaded.listWatchedWallets("chat-1"), []);

    const body = JSON.parse(await readFile(path, "utf8"));
    assert.equal(body.subscribers[0].chatId, "chat-1");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

function heliusSwapEvent(overrides = {}) {
  return {
    type: "SWAP",
    source: "JUPITER",
    feePayer: wallet,
    signature: "5VfUXexampleTxSignature111111111111111111111111111111111",
    timestamp: 1770000000,
    nativeTransfers: [
      {
        fromUserAccount: wallet,
        toUserAccount: "Fee111111111111111111111111111111111111111",
        amount: 125000000
      }
    ],
    tokenTransfers: [
      {
        fromUserAccount: "Pool111111111111111111111111111111111111111",
        toUserAccount: wallet,
        mint,
        tokenAmount: 250000,
        symbol: "BONK"
      }
    ],
    ...overrides
  };
}

test("wallet monitor normalizes and formats matching Helius swaps", () => {
  const event = {
    ...heliusSwapEvent()
  };

  assert.equal(isValidSolanaAddress(wallet), true);
  assert.equal(heliusEventMentionsWatchedWallet(event, wallet), true);
  assert.equal(heliusEventMentionsWatchedWallet(event, otherWallet), false);

  const trade = normalizeHeliusSwapData({
    event,
    targetWallet: wallet,
    label: "Alpha <Wallet>",
    config
  });

  assert.equal(trade.provider, "helius");
  assert.equal(trade.action, "buy");
  assert.equal(trade.mint, mint);
  assert.equal(trade.solAmount, 0.125);
  assert.equal(trade.tokenAmount, 250000);
  assert.deepEqual(trade.input, {
    mint: "So11111111111111111111111111111111111111112",
    symbol: "SOL",
    amount: 0.125
  });
  assert.deepEqual(trade.output, {
    mint,
    symbol: "BONK",
    amount: 250000
  });
  assert.equal(getWalletTradeEventId(trade), `wallet-trade:helius:${trade.signature}:${wallet}`);
  assert.deepEqual(buildWalletTradeReplyMarkup(trade)?.inline_keyboard[0][0].copy_text, { text: mint });

  const message = formatWalletTradeMessage(trade);
  assert.match(message, /Wallet trade detected/);
  assert.match(message, /Alpha &lt;Wallet&gt;/);
  assert.match(message, /0.125 SOL -> 250,000 BONK/);
  assert.match(message, /0.125 SOL/);
  assert.match(message, /Source:<\/b> JUPITER/);
});

test("copy candidate formatting and missing wallet build skip", () => {
  const trade = normalizeHeliusSwapData({
    event: heliusSwapEvent(),
    targetWallet: wallet,
    label: "Alpha <Wallet>",
    config
  });
  const subscriber = {
    chatId: "chat-1",
    mode: "both",
    watchedWallets: [],
    copyWallet: null,
    copyWallets: [],
    copySolAmount: null,
    copySettingsUpdatedAt: null,
    verifiedAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  };
  const build = buildPumpPortalLocalTradeRequest({ trade, subscriber, config });
  const message = formatCopyCandidateMessage({
    trade,
    copySolAmount: copySolAmountForSubscriber(subscriber, config),
    build
  });

  assert.equal(isCopyCandidateTrade(trade), true);
  assert.equal(build.status, "skipped");
  assert.equal(build.message, "PumpPortal build skipped: set /copywallet");
  assert.match(message, /Copy candidate/);
  assert.match(message, /Alpha &lt;Wallet&gt;/);
  assert.match(message, /Target spent:<\/b> 0.125 SOL/);
  assert.match(message, /Your copy amount:<\/b> 0.01 SOL/);
  assert.match(message, /Build-only: not signed, not sent/);
});

test("PumpPortal Local request and build result stay unsigned", async () => {
  const trade = normalizeHeliusSwapData({
    event: heliusSwapEvent(),
    targetWallet: wallet,
    config
  });
  const subscriber = {
    chatId: "chat-1",
    mode: "both",
    watchedWallets: [],
    copyWallet: otherWallet,
    copyWallets: [otherWallet],
    copySolAmount: 0.02,
    copySettingsUpdatedAt: new Date().toISOString(),
    verifiedAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  };
  const planned = buildPumpPortalLocalTradeRequest({ trade, subscriber, config });

  assert.deepEqual(planned.request, {
    publicKey: otherWallet,
    action: "buy",
    mint,
    amount: 0.02,
    denominatedInSol: "true",
    slippage: 10,
    priorityFee: 0.00005,
    pool: "auto"
  });

  const calls = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({
      url: String(url),
      body: JSON.parse(String(init.body))
    });
    return new Response(new Uint8Array([1, 2, 3, 4]), {
      status: 200,
      headers: {
        "content-type": "application/octet-stream"
      }
    });
  };

  try {
    const built = await buildPumpPortalLocalTransaction({ trade, subscriber, config });

    assert.equal(built.status, "built");
    assert.equal(built.responseBytes, 4);
    assert.equal(built.responseStatus, 200);
    assert.equal(calls[0].url, config.pumpPortalLocalTradeUrl);
    assert.deepEqual(calls[0].body, planned.request);
    assert.equal("encodedTransactionBase64" in built, false);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("PumpPortal Local 400 response is surfaced clearly", async () => {
  const trade = normalizeHeliusSwapData({
    event: heliusSwapEvent(),
    targetWallet: wallet,
    config
  });
  const subscriber = {
    chatId: "chat-1",
    mode: "both",
    watchedWallets: [],
    copyWallet: otherWallet,
    copyWallets: [otherWallet],
    copySolAmount: 0.02,
    copySettingsUpdatedAt: new Date().toISOString(),
    verifiedAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  };
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response("Bad Request", {
      status: 400,
      headers: {
        "content-type": "text/plain"
      }
    });

  try {
    const built = await buildPumpPortalLocalTransaction({ trade, subscriber, config });

    assert.equal(built.status, "failed");
    assert.equal(built.responseStatus, 400);
    assert.match(built.message, /HTTP 400 Bad Request/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("PumpPortal Local builds one unsigned transaction per copy wallet", async () => {
  const trade = normalizeHeliusSwapData({
    event: heliusSwapEvent(),
    targetWallet: wallet,
    config
  });
  const subscriber = {
    chatId: "chat-1",
    mode: "both",
    watchedWallets: [],
    copyWallet: otherWallet,
    copyWallets: [otherWallet, wallet],
    copySolAmount: 0.02,
    copySettingsUpdatedAt: new Date().toISOString(),
    verifiedAt: new Date().toISOString(),
    updatedAt: new Date().toISOString()
  };
  const calls = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({
      url: String(url),
      body: JSON.parse(String(init.body))
    });
    return new Response(new Uint8Array([1, 2]), {
      status: 200,
      headers: {
        "content-type": "application/octet-stream"
      }
    });
  };

  try {
    const built = await buildPumpPortalLocalTransactions({ trade, subscriber, config });

    assert.equal(built.length, 2);
    assert.deepEqual(
      calls.map((call) => call.body.publicKey),
      [otherWallet, wallet]
    );
    assert.deepEqual(
      built.map((entry) => entry.build.status),
      ["built", "built"]
    );
    assert.match(formatCopyCandidateMessage({ trade, builds: built }), /Copy wallets:<\/b> 2/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("Telegram copy commands require verification and save copy amount", async () => {
  const store = createSubscriberStore({});
  await store.init();

  const unverified = await runOneTelegramCommand(store, "/copywallet 62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV");
  assert.match(unverified, /Verification required/);

  await store.add("chat-1");
  const amountReply = await runOneTelegramCommand(store, "/copyamount 0.02");
  assert.match(amountReply, /Copy amount set/);
  assert.equal(store.get("chat-1").copySolAmount, 0.02);

  const walletReply = await runOneTelegramCommand(store, `/copywallet ${otherWallet}`);
  assert.match(walletReply, /Copy wallet added/);
  assert.equal(store.get("chat-1").copyWallet, otherWallet);
  assert.deepEqual(store.get("chat-1").copyWallets, [otherWallet]);

  const statusReply = await runOneTelegramCommand(store, "/copystatus");
  assert.match(statusReply, /Copy dry-run status/);
  assert.match(statusReply, /Copy wallets:<\/b> 1/);
  assert.match(statusReply, /0.02 SOL/);
});

test("Telegram copy wallet commands list and remove multiple copy wallets", async () => {
  const store = createSubscriberStore({});
  await store.init();
  await store.add("chat-1");

  await runOneTelegramCommand(store, `/copywallet ${otherWallet}`);
  await runOneTelegramCommand(store, `/copywallet ${wallet}`);
  assert.deepEqual(store.listCopyWallets("chat-1"), [otherWallet, wallet]);

  const listReply = await runOneTelegramCommand(store, "/copywallets");
  assert.match(listReply, /Copy wallets/);
  assert.match(listReply, new RegExp(otherWallet));
  assert.match(listReply, new RegExp(wallet));

  const removeReply = await runOneTelegramCommand(store, `/uncopywallet ${wallet}`);
  assert.match(removeReply, /Copy wallet removed/);
  assert.deepEqual(store.listCopyWallets("chat-1"), [otherWallet]);
});

test("Telegram copytest requires verification and watched wallets", async () => {
  const store = createSubscriberStore({});
  await store.init();

  const unverified = await runOneTelegramCommand(store, "/copytest");
  assert.match(unverified, /Verification required/);

  await store.add("chat-1");
  const noWallets = await runOneTelegramCommand(store, "/copytest");
  assert.match(noWallets, /No watched wallets/);
  assert.match(noWallets, /\/watch wallet-address optional-label/);
});

test("Telegram copytest calls scanner for watched wallets", async () => {
  const store = createSubscriberStore({});
  await store.init();
  await store.add("chat-1");
  await store.watchWallet("chat-1", wallet, "Alpha");

  const calls = [];
  const reply = await runOneTelegramCommand(store, "/copytest", {
    onCopyTest: (chatId) => {
      calls.push(chatId);
      return "Copy test scanned 1 wallet(s), 20 recent swap(s), sent 1 candidate alert(s).";
    }
  });

  assert.deepEqual(calls, ["chat-1"]);
  assert.match(reply, /Copy test scanned 1 wallet/);
  assert.match(reply, /sent 1 candidate alert/);
});

async function runOneTelegramCommand(store, text, options = {}) {
  const originalFetch = globalThis.fetch;
  const sent = [];
  let servedUpdates = false;
  let poller;

  globalThis.fetch = async (url, init) => {
    const urlText = String(url);

    if (urlText.includes("/getMe")) {
      return telegramJson({ username: "copy_bot" });
    }

    if (urlText.includes("/deleteWebhook") || urlText.includes("/setMyCommands")) {
      return telegramJson(true);
    }

    if (urlText.includes("/getUpdates")) {
      if (servedUpdates) {
        poller?.stop();
        return telegramJson([]);
      }

      servedUpdates = true;
      return telegramJson([
        {
          update_id: 1,
          message: {
            text,
            chat: {
              id: "chat-1"
            }
          }
        }
      ]);
    }

    if (urlText.includes("/sendMessage")) {
      const body = JSON.parse(String(init.body));
      sent.push(body.text);
      poller?.stop();
      return telegramJson({});
    }

    throw new Error(`Unexpected Telegram API call: ${urlText}`);
  };

  try {
    poller = createTelegramCommandPoller({
      config: {
        telegramToken: "telegram-token",
        telegramVerifyCode: "secret",
        pumpPortalWsUrl: "wss://pumpportal.test",
        copyDefaultSolAmount: 0.01,
        pumpFunBaseUrl: "https://pump.fun",
        solscanBaseUrl: "https://solscan.io"
      },
      testMessage: () => "",
      subscribers: store,
      onCopyTest: options.onCopyTest
    });
    await poller.start();
    return sent[0] || "";
  } finally {
    poller?.stop();
    globalThis.fetch = originalFetch;
  }
}

function telegramJson(result) {
  return new Response(JSON.stringify({ ok: true, result }), {
    status: 200,
    headers: {
      "content-type": "application/json"
    }
  });
}

test("Helius swap normalization handles token to SOL and token to token swaps", () => {
  const tokenToSol = normalizeHeliusSwapData({
    event: heliusSwapEvent({
      nativeTransfers: [
        {
          fromUserAccount: "Pool111111111111111111111111111111111111111",
          toUserAccount: wallet,
          amount: 1500000000
        }
      ],
      tokenTransfers: [
        {
          fromUserAccount: wallet,
          toUserAccount: "Pool111111111111111111111111111111111111111",
          mint,
          tokenAmount: 100,
          symbol: "BONK"
        }
      ]
    }),
    targetWallet: wallet,
    config
  });
  const tokenToToken = normalizeHeliusSwapData({
    event: heliusSwapEvent({
      nativeTransfers: [],
      tokenTransfers: [
        {
          fromUserAccount: wallet,
          toUserAccount: "Pool111111111111111111111111111111111111111",
          mint,
          tokenAmount: 100,
          symbol: "BONK"
        },
        {
          fromUserAccount: "Pool222222222222222222222222222222222222222",
          toUserAccount: wallet,
          mint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY6JqTd6FJqyf2s",
          tokenAmount: 12.5,
          symbol: "USDT"
        }
      ]
    }),
    targetWallet: wallet,
    config
  });

  assert.deepEqual(tokenToSol.input, { mint, symbol: "BONK", amount: 100 });
  assert.deepEqual(tokenToSol.output, {
    mint: "So11111111111111111111111111111111111111112",
    symbol: "SOL",
    amount: 1.5
  });
  assert.deepEqual(tokenToToken.input, { mint, symbol: "BONK", amount: 100 });
  assert.deepEqual(tokenToToken.output, {
    mint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY6JqTd6FJqyf2s",
    symbol: "USDT",
    amount: 12.5
  });
  assert.equal(isCopyCandidateTrade(tokenToSol), false);
  assert.equal(isCopyCandidateTrade(tokenToToken), false);
});

test("Helius webhook payload uses enhanced SWAP config", () => {
  assert.deepEqual(
    buildHeliusWebhookPayload({
      accountAddresses: [wallet],
      authHeader: "Bearer secret",
      publicUrl: "https://example.com/webhooks/helius"
    }),
    {
      webhookURL: "https://example.com/webhooks/helius",
      transactionTypes: ["SWAP"],
      accountAddresses: [wallet],
      webhookType: "enhanced",
      authHeader: "Bearer secret",
      txnStatus: "success"
    }
  );
});

test("Helius webhook sync creates and updates request bodies", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pumpfunnoti-helius-"));
  const statePath = join(dir, "helius-webhook.json");
  const requests = [];
  const originalFetch = globalThis.fetch;

  globalThis.fetch = async (url, init) => {
    requests.push({
      url: String(url),
      method: init?.method,
      body: JSON.parse(String(init?.body))
    });
    return {
      ok: true,
      json: async () => ({ webhookID: requests.length === 1 ? "created-id" : "created-id" })
    };
  };

  try {
    const first = await syncHeliusWebhook({
      apiKey: "key",
      apiBaseUrl: "https://helius.test",
      authHeader: "Bearer secret",
      publicUrl: "https://example.com/webhooks/helius",
      statePath,
      accountAddresses: [wallet]
    });
    const second = await syncHeliusWebhook({
      apiKey: "key",
      apiBaseUrl: "https://helius.test",
      authHeader: "Bearer secret",
      publicUrl: "https://example.com/webhooks/helius",
      statePath,
      accountAddresses: [wallet, otherWallet]
    });

    assert.equal(first.webhookId, "created-id");
    assert.equal(second.webhookId, "created-id");
    assert.equal(requests[0].method, "POST");
    assert.equal(requests[1].method, "PUT");
    assert.match(requests[1].url, /\/v0\/webhooks\/created-id\?/);
    assert.deepEqual(requests[1].body.accountAddresses, [wallet, otherWallet]);
  } finally {
    globalThis.fetch = originalFetch;
    await rm(dir, { recursive: true, force: true });
  }
});

test("Helius webhook sync allows zero watched wallets", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pumpfunnoti-helius-empty-"));
  const statePath = join(dir, "helius-webhook.json");
  const originalFetch = globalThis.fetch;
  let fetchCalls = 0;

  globalThis.fetch = async () => {
    fetchCalls += 1;
    throw new Error("empty watchlist should not call Helius");
  };

  try {
    const withoutWebhook = await syncHeliusWebhook({
      apiKey: "key",
      apiBaseUrl: "https://helius.test",
      authHeader: "Bearer secret",
      publicUrl: "https://example.com/webhooks/helius",
      statePath,
      accountAddresses: []
    });

    await writeFile(statePath, `${JSON.stringify({ webhookID: "existing-id" })}\n`);

    const withWebhook = await syncHeliusWebhook({
      apiKey: "key",
      apiBaseUrl: "https://helius.test",
      authHeader: "Bearer secret",
      publicUrl: "https://example.com/webhooks/helius",
      statePath,
      accountAddresses: []
    });

    assert.equal(fetchCalls, 0);
    assert.deepEqual(withoutWebhook, {
      ok: true,
      webhookId: null,
      skipped: true,
      message: "No watched wallets; Helius webhook not created."
    });
    assert.deepEqual(withWebhook, {
      ok: true,
      webhookId: "existing-id",
      skipped: true,
      message: "No watched wallets; leaving existing Helius webhook unchanged."
    });
  } finally {
    globalThis.fetch = originalFetch;
    await rm(dir, { recursive: true, force: true });
  }
});

test("Helius webhook receiver enforces Authorization", async () => {
  const received = [];
  const server = createHeliusWebhookServer({
    authHeader: "Bearer secret",
    port: 0,
    onEvents: (events) => {
      received.push(...events);
    }
  });

  await server.start();
  const port = server.port();
  assert.ok(port);

  try {
    const unauthorized = await fetch(`http://127.0.0.1:${port}/webhooks/helius`, {
      method: "POST",
      headers: {
        "content-type": "application/json"
      },
      body: JSON.stringify([heliusSwapEvent()])
    });

    assert.equal(unauthorized.status, 401);

    const authorized = await fetch(`http://127.0.0.1:${port}/webhooks/helius`, {
      method: "POST",
      headers: {
        authorization: "Bearer secret",
        "content-type": "application/json"
      },
      body: JSON.stringify([heliusSwapEvent()])
    });

    assert.equal(authorized.status, 200);
    await new Promise((resolve) => setTimeout(resolve, 10));
    assert.equal(received.length, 1);
  } finally {
    await server.stop();
  }
});
