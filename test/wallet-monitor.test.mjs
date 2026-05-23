import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { commandFromMessage, helpText, toggleAlertMode } from "../dist/commands.js";
import { buildHeliusWebhookPayload, createHeliusWebhookServer, syncHeliusWebhook } from "../dist/helius.js";
import { heliusEventMentionsWatchedWallet, normalizeHeliusSwapData } from "../dist/helius-swaps.js";
import {
  buildPumpPortalLightningBuyRequest,
  buildPumpPortalLocalSellRequest,
  buildPumpPortalLocalTradeRequest,
  createPumpPortalLightningWallet,
  executePumpPortalLightningTrade
} from "../dist/pumpportal.js";
import { decryptSecret, encryptSecret } from "../dist/secrets.js";
import { createSubscriberStore } from "../dist/subscribers.js";
import {
  buildWalletTradeReplyMarkup,
  formatAutoCopyBuyMessage,
  formatCopyTradeTrailingSellBuildMessage,
  formatCopyTradeTrailingSellScheduledMessage,
  formatCopyTradeSimulationMessage,
  formatWalletTradeMessage,
  formatWalletTradeMessageWithCopySettings,
  getWalletTradeEventId,
  isCopyableSolToTokenBuy,
  isValidSolanaAddress
} from "../dist/wallet-monitor.js";

const wallet = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
const otherWallet = "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV";
const mint = "DezXAZ8z7PnrnRJjz3n26VsZdrmRwNny4nBj9JkzjW7B";
const config = {
  pumpFunBaseUrl: "https://pump.fun",
  solscanBaseUrl: "https://solscan.io"
};
const encryptionSecret = "abcdefghijklmnopqrstuvwxyz1234567890";

test("telegram help exposes wallet and copy trade dashboards", () => {
  const help = helpText("chat-1");

  assert.match(help, /\/alerts - Open alert mode dashboard/);
  assert.match(help, /\/trackwallets - Open track wallet dashboard/);
  assert.match(help, /\/mywallets - Open trading wallet dashboard/);
  assert.match(help, /\/copytrade - Open copy trade setup menu/);
  assert.doesNotMatch(help, /\/wallets/);
  assert.doesNotMatch(help, /\/migrations/);
  assert.doesNotMatch(help, /\/newtokens/);
  assert.doesNotMatch(help, /\/both/);
  assert.doesNotMatch(help, /\/watch/);
  assert.doesNotMatch(help, /\/renamewallet/);
  assert.doesNotMatch(help, /\/unwatch/);
  assert.doesNotMatch(help, /\/copywallet/);
  assert.doesNotMatch(help, /\/copyamount/);
  assert.doesNotMatch(help, /\/copystatus/);
  assert.deepEqual(commandFromMessage({ text: "/trackwallets" }), {
    command: "/trackwallets",
    args: []
  });
  assert.deepEqual(commandFromMessage({ text: "/mywallets" }), {
    command: "/mywallets",
    args: []
  });
  assert.deepEqual(commandFromMessage({ text: "/alerts" }), {
    command: "/alerts",
    args: []
  });
  assert.deepEqual(commandFromMessage({ text: "/migrations" }), {
    command: "/migrations",
    args: []
  });
  assert.deepEqual(commandFromMessage({ text: "/copytrade" }), {
    command: "/copytrade",
    args: []
  });
});

test("alert mode toggles individual token alert types", () => {
  assert.equal(toggleAlertMode("both", "migrations"), "newtokens");
  assert.equal(toggleAlertMode("both", "newtokens"), "migrations");
  assert.equal(toggleAlertMode("migrations", "migrations"), null);
  assert.equal(toggleAlertMode("newtokens", "migrations"), "both");
  assert.equal(toggleAlertMode(null, "migrations"), "migrations");
  assert.equal(toggleAlertMode(null, "newtokens"), "newtokens");
});

test("PumpPortal Lightning wallet helpers parse, encrypt, and execute requests", async () => {
  const encrypted = encryptSecret("secret-api-key", encryptionSecret);
  assert.equal(decryptSecret(encrypted, encryptionSecret), "secret-api-key");
  assert.throws(() => decryptSecret(encrypted, "wrong-secret-wrong-secret-wrong-secret"));
  assert.throws(() => encryptSecret("secret-api-key", "short"));

  const originalFetch = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async (url, init) => {
    calls.push({ url: String(url), init });

    if (String(url).includes("create-wallet")) {
      return new Response(
        JSON.stringify({
          publicKey: wallet,
          privateKey: "private-key-alpha",
          apiKey: "api-key-alpha"
        }),
        { status: 200 }
      );
    }

    return new Response(JSON.stringify({ signature: "tx-alpha" }), { status: 200 });
  };

  try {
    const created = await createPumpPortalLightningWallet({ url: "https://pumpportal.fun/api/create-wallet" });
    assert.equal(created.ok, true);
    assert.equal(created.wallet.publicKey, wallet);
    assert.equal(created.wallet.privateKey, "private-key-alpha");
    assert.equal(created.wallet.apiKey, "api-key-alpha");

    const request = {
      action: "buy",
      mint,
      amount: 0.25,
      denominatedInSol: "true",
      slippage: 15,
      priorityFee: 0.00009,
      pool: "auto"
    };
    const result = await executePumpPortalLightningTrade({
      url: "https://pumpportal.fun/api/trade",
      apiKey: "api-key-alpha",
      request
    });

    assert.equal(result.ok, true);
    assert.equal(result.signature, "tx-alpha");
    assert.match(calls[1].url, /api-key=api-key-alpha/);
    assert.deepEqual(JSON.parse(calls[1].init.body), request);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

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
    assert.equal(await store.renameWallet("chat-1", wallet, "Alpha <Wallet>"), true);
    assert.equal(await store.setMode("chat-1", "newtokens"), true);
    assert.equal(store.get("chat-1")?.mode, "newtokens");
    assert.equal(await store.setMode("chat-1", null), true);
    assert.equal(store.get("chat-1")?.mode, null);
    assert.equal(await store.setMode("chat-2", "both"), false);
    assert.equal(await store.setCopyWallet("chat-1", otherWallet), true);
    assert.equal(await store.setCopyWallet("chat-1", wallet), true);
    assert.deepEqual(store.listCopyWallets("chat-1"), [otherWallet, wallet]);
    assert.equal(await store.setCopyAmountSol("chat-1", 0.25), true);
    assert.equal(await store.addMyWallet("chat-1", otherWallet, "My Alpha"), true);
    assert.equal(await store.addMyWallet("chat-1", wallet, "My Beta"), true);
    assert.equal(await store.renameMyWallet("chat-1", wallet, null), true);
    assert.equal(await store.renameMyWallet("chat-1", "missing", "Nope"), false);
    assert.equal(await store.addMyWallet("chat-2", wallet, "Unverified"), false);
    assert.equal(
      await store.setTradingWallet("chat-1", {
        publicKey: otherWallet,
        encryptedApiKey: encryptSecret("pump-key-alpha", encryptionSecret),
        apiKeyLast4: "lpha",
        createdAt: "2026-05-23T00:00:00.000Z",
        updatedAt: "2026-05-23T00:00:00.000Z"
      }),
      true
    );
    assert.equal(await store.setTradingWallet("chat-2", store.getTradingWallet("chat-1")), false);
    assert.equal(await store.watchCopyTradeWallet("chat-1", otherWallet, "Copy Alpha"), true);
    assert.equal(await store.watchCopyTradeWallet("chat-1", wallet, "Copy Beta"), true);
    assert.equal(await store.renameCopyTradeWallet("chat-1", wallet, "Copy Gamma"), true);
    assert.equal(await store.renameCopyTradeWallet("chat-1", wallet, null), true);
    assert.equal(await store.renameCopyTradeWallet("chat-1", "missing", "Nope"), false);
    assert.equal(await store.watchCopyTradeWallet("chat-2", wallet, "Unverified"), false);
    assert.equal(await store.setCopyWallet("chat-2", otherWallet), false);
    assert.equal(await store.setCopyAmountSol("chat-2", 0.25), false);
    assert.equal(store.get("chat-1")?.copyWalletAddress, null);
    assert.deepEqual(store.get("chat-1")?.copyWalletAddresses, []);
    assert.deepEqual(store.listCopyWallets("chat-1"), []);
    assert.deepEqual(
      store.listMyWallets("chat-1").map((entry) => [entry.address, entry.label]),
      [
        [wallet, null],
        [otherWallet, "My Alpha"]
      ]
    );
    assert.equal(store.get("chat-1")?.copyAmountSol, 0.25);
    assert.equal(store.getTradingWallet("chat-1")?.publicKey, otherWallet);
    assert.equal(decryptSecret(store.getTradingWallet("chat-1")?.encryptedApiKey || "", encryptionSecret), "pump-key-alpha");
    assert.deepEqual(
      store.listCopyTradeWallets("chat-1").map((entry) => [entry.address, entry.label]),
      [
        [wallet, null],
        [otherWallet, "Copy Alpha"]
      ]
    );

    const reloaded = createSubscriberStore({ path });
    await reloaded.init();
    assert.deepEqual(
      reloaded.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
      [[wallet, "Alpha <Wallet>"]]
    );
    assert.equal(reloaded.get("chat-1")?.copyWalletAddress, null);
    assert.deepEqual(reloaded.get("chat-1")?.copyWalletAddresses, []);
    assert.deepEqual(
      reloaded.listMyWallets("chat-1").map((entry) => [entry.address, entry.label]),
      [
        [wallet, null],
        [otherWallet, "My Alpha"]
      ]
    );
    assert.equal(reloaded.get("chat-1")?.copyAmountSol, 0.25);
    assert.equal(reloaded.getTradingWallet("chat-1")?.publicKey, otherWallet);
    assert.deepEqual(
      reloaded.listCopyTradeWallets("chat-1").map((entry) => [entry.address, entry.label]),
      [
        [wallet, null],
        [otherWallet, "Copy Alpha"]
      ]
    );
    assert.equal(reloaded.get("chat-1")?.mode, null);

    assert.equal(await reloaded.unwatchWallet("chat-1", wallet), true);
    assert.deepEqual(reloaded.listWatchedWallets("chat-1"), []);
    assert.deepEqual(
      reloaded.listCopyTradeWallets("chat-1").map((entry) => entry.address),
      [wallet, otherWallet]
    );
    assert.equal(await reloaded.unwatchCopyTradeWallet("chat-1", wallet), true);
    assert.deepEqual(reloaded.listCopyTradeWallets("chat-1").map((entry) => entry.address), [otherWallet]);
    assert.equal(await reloaded.removeMyWallet("chat-1", wallet), true);
    assert.deepEqual(reloaded.listMyWallets("chat-1").map((entry) => entry.address), [otherWallet]);

    const body = JSON.parse(await readFile(path, "utf8"));
    assert.equal(body.subscribers[0].chatId, "chat-1");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("subscriber store migrates legacy copy target out of watched wallets", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pumpfunnoti-legacy-copytrade-"));
  const path = join(dir, "subscribers.json");

  try {
    await writeFile(
      path,
      JSON.stringify({
        subscribers: [
          {
            chatId: "chat-1",
            mode: "both",
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
            copyTargetWalletAddress: wallet,
            verifiedAt: "2026-05-22T00:00:00.000Z",
            updatedAt: "2026-05-22T00:00:00.000Z"
          }
        ]
      })
    );

    const store = createSubscriberStore({ path });
    await store.init();

    assert.deepEqual(store.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]), [[otherWallet, "Normal Watch"]]);
    assert.deepEqual(store.listCopyTradeWallets("chat-1").map((entry) => [entry.address, entry.label]), [[wallet, "Legacy Copy"]]);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("subscriber store init does not create a file from seeded chat ids", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pumpfunnoti-init-"));
  const path = join(dir, "subscribers.json");

  try {
    const store = createSubscriberStore({ path, initialChatIds: ["seed-chat"] });
    await store.init();
    assert.equal(store.has("seed-chat"), true);
    await assert.rejects(stat(path), { code: "ENOENT" });
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

  const copyMessage = formatWalletTradeMessageWithCopySettings(trade, {
    copyWalletAddress: otherWallet,
    copyWalletAddresses: [otherWallet, wallet],
    copyAmountSol: 0.25
  });
  assert.match(copyMessage, /Copy trade/);
  assert.match(copyMessage, new RegExp(otherWallet));
  assert.match(copyMessage, new RegExp(wallet));
  assert.match(copyMessage, /Copy wallets:<\/b> 2/);
  assert.match(copyMessage, /Copy amount:<\/b> 0.25 SOL/);
  assert.match(copyMessage, /Status:<\/b> Ready to copy 0.25 SOL into this token from 2 wallet/);

  assert.equal(isCopyableSolToTokenBuy(trade), true);
  const simulationMessage = formatCopyTradeSimulationMessage(trade, {
    copyWalletAddress: otherWallet,
    copyAmountSol: 0.25
  });
  assert.match(simulationMessage || "", /Would copy trade/);
  assert.match(simulationMessage || "", /Alpha &lt;Wallet&gt;/);
  assert.match(simulationMessage || "", new RegExp(otherWallet));
  assert.match(simulationMessage || "", new RegExp(mint));
  assert.match(simulationMessage || "", /Would buy this token with 0.25 SOL/);
  assert.match(simulationMessage || "", /PumpPortal:<\/b> Local transaction build not requested/);
  assert.doesNotMatch(simulationMessage || "", /500,000/);

  const builtSimulationMessage = formatCopyTradeSimulationMessage(
    trade,
    {
      copyWalletAddress: otherWallet,
      copyAmountSol: 0.25
    },
    {
      ok: true,
      status: 200,
      bodyLength: 1234,
      errorText: null
    }
  );
  assert.match(builtSimulationMessage || "", /PumpPortal:<\/b> Local transaction built \(1,234 bytes\)/);

  assert.deepEqual(
    buildPumpPortalLocalTradeRequest({
      trade,
      copySettings: {
        copyWalletAddress: otherWallet,
        copyAmountSol: 0.25
      },
      slippage: 15,
      priorityFee: 0.00009,
      pool: "auto"
    }),
    {
      publicKey: otherWallet,
      action: "buy",
      mint,
      amount: 0.25,
      denominatedInSol: "true",
      slippage: 15,
      priorityFee: 0.00009,
      pool: "auto"
    }
  );

  assert.deepEqual(
    buildPumpPortalLightningBuyRequest({
      trade,
      amountSol: 0.25,
      slippage: 15,
      priorityFee: 0.00009,
      pool: "auto"
    }),
    {
      action: "buy",
      mint,
      amount: 0.25,
      denominatedInSol: "true",
      slippage: 15,
      priorityFee: 0.00009,
      pool: "auto"
    }
  );

  const autoBuyMessage = formatAutoCopyBuyMessage({
    trade,
    tradingWalletPublicKey: otherWallet,
    copyAmountSol: 0.25,
    result: {
      ok: true,
      status: 200,
      signature: "tx-alpha",
      errorText: null,
      raw: { signature: "tx-alpha" }
    }
  });
  assert.match(autoBuyMessage || "", /Auto copy buy submitted/);
  assert.match(autoBuyMessage || "", /Submitted: <code>tx-alpha<\/code>/);
  assert.match(autoBuyMessage || "", new RegExp(otherWallet));

  const sellRequest = buildPumpPortalLocalSellRequest({
    publicKey: otherWallet,
    mint,
    amountPercent: 20,
    slippage: 15,
    priorityFee: 0.00009,
    pool: "auto"
  });

  assert.deepEqual(sellRequest, {
    publicKey: otherWallet,
    action: "sell",
    mint,
    amount: "20%",
    denominatedInSol: "false",
    slippage: 15,
    priorityFee: 0.00009,
    pool: "auto"
  });

  const trailingScheduledMessage = formatCopyTradeTrailingSellScheduledMessage({
    trade,
    copyWalletAddress: otherWallet,
    steps: [
      {
        delayMs: 2000,
        request: sellRequest
      },
      {
        delayMs: 4000,
        request: {
          ...sellRequest,
          amount: "100%"
        }
      }
    ]
  });
  assert.match(trailingScheduledMessage || "", /Trailing sell scheduled/);
  assert.match(trailingScheduledMessage || "", /Sell 20% after 2s/);
  assert.match(trailingScheduledMessage || "", /Sell 100% after 4s/);
  assert.match(trailingScheduledMessage || "", /Build-only: not signed, not sent/);

  const trailingBuildMessage = formatCopyTradeTrailingSellBuildMessage({
    trade,
    copyWalletAddress: otherWallet,
    stepIndex: 1,
    totalSteps: 2,
    request: {
      ...sellRequest,
      amount: "100%"
    },
    pumpPortalBuild: {
      ok: true,
      status: 200,
      bodyLength: 555,
      errorText: null
    }
  });
  assert.match(trailingBuildMessage, /Trailing sell build/);
  assert.match(trailingBuildMessage, /Step:<\/b> 2\/2/);
  assert.match(trailingBuildMessage, /Sell amount:<\/b> 100%/);
  assert.match(trailingBuildMessage, /Local transaction built \(555 bytes\)/);
});

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
  assert.equal(isCopyableSolToTokenBuy(tokenToSol), false);
  assert.equal(isCopyableSolToTokenBuy(tokenToToken), false);
  assert.equal(
    formatCopyTradeSimulationMessage(tokenToSol, {
      copyWalletAddress: otherWallet,
      copyAmountSol: 0.1
    }),
    null
  );
  assert.equal(
    formatCopyTradeSimulationMessage(tokenToToken, {
      copyWalletAddress: otherWallet,
      copyAmountSol: 0.1
    }),
    null
  );
  assert.equal(
    formatCopyTradeSimulationMessage(tokenToToken, {
      copyWalletAddress: null,
      copyAmountSol: 0.1
    }),
    null
  );
  assert.match(
    formatWalletTradeMessageWithCopySettings(tokenToSol, {
      copyWalletAddress: otherWallet,
      copyAmountSol: 0.1
    }),
    /Status:<\/b> Not a copyable SOL-to-token buy/
  );
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
