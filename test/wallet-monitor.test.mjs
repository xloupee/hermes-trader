import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  commandFromMessage,
  createTelegramCommandPoller,
  formatCopyTradeDashboardText,
  formatStartDashboardText,
  helpText,
  parsePriorityFeeInput,
  parseSlippageInput,
  parseTrailingSellFormulaInput,
  parseTrailingSellStepInput,
  parseTrailingSellStepsInput,
  toggleAlertMode
} from "../dist/commands.js";
import { buildHeliusWebhookPayload, createHeliusWebhookServer, syncHeliusWebhook } from "../dist/helius.js";
import { heliusEventMentionsWatchedWallet, normalizeHeliusSwapData } from "../dist/helius-swaps.js";
import {
  buildPumpPortalLightningBuyRequest,
  buildPumpPortalLightningSellRequest,
  buildPumpPortalLocalTradeRequest,
  createPumpPortalLightningWallet,
  executePumpPortalLightningTrade
} from "../dist/pumpportal.js";
import { decryptSecret, encryptSecret } from "../dist/secrets.js";
import { createSubscriberStore } from "../dist/subscribers.js";
import {
  buildWalletTradeReplyMarkup,
  formatAutoCopyBuyMessage,
  formatCopyTradeTrailingSellResultMessage,
  formatCopyTradeTrailingSellSkippedMessage,
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

  assert.match(help, /📚 Pump\.fun Notifier Help/);
  assert.match(help, /🚀 Quick Start/);
  assert.match(help, /\/alerts - Toggle migrated coins and new tokens/);
  assert.match(help, /\/trackwallets - Track wallets for normal trade alerts/);
  assert.match(help, /\/mywallets - Create or view your local signing trading wallet/);
  assert.match(help, /\/copytrade - Configure copy amount, wallets, and trailing sells/);
  assert.match(help, /\/cashback - View and claim platform-fee cashback/);
  assert.doesNotMatch(help, /🕒 Last updated:/);
  assert.doesNotMatch(help, /Commands:/);
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
  assert.deepEqual(commandFromMessage({ text: "/cashback" }), {
    command: "/cashback",
    args: []
  });
});

test("start dashboard uses polished status card", () => {
  const dashboard = formatStartDashboardText({
    chatId: "chat-1",
    mode: "both",
    watchedWallets: [{ address: wallet, label: "Alpha", addedAt: "now", updatedAt: "now" }],
    copyTradeWallets: [{ address: otherWallet, label: "Cented", addedAt: "now", updatedAt: "now" }],
    tradingWallet: {
      publicKey: "9yQC6vxwibseQtcRQGm7z6ymYiBGkRkKg3DnMZzRpy1i",
      encryptedApiKey: "encrypted",
      apiKeyLast4: "abcd",
      createdAt: "now",
      updatedAt: "now"
    },
    copyWalletAddress: null,
    copyWalletAddresses: [],
    copyAmountSol: 0.5,
    copyTradeBuySlippagePercent: null,
    copyTradeBuyPriorityFeeSol: null,
    copyTradeSellSlippagePercent: null,
    copyTradeSellPriorityFeeSol: null,
    copyTargetWalletAddress: null,
    verifiedAt: "now",
    updatedAt: "now"
  });

  assert.match(dashboard, /🚀 Welcome to Pump\.fun Notifier/);
  assert.match(dashboard, /🟢 Setup is <b>active<\/b>/);
  assert.match(dashboard, /🔔 Token Alerts/);
  assert.match(dashboard, /👀 Tracked Wallets/);
  assert.match(dashboard, /👛 Trading Wallet/);
  assert.match(dashboard, /⚡ Copy Trading/);
  assert.match(dashboard, /\/cashback - Copytrade cashback/);
  assert.match(dashboard, /Cented/);
  assert.doesNotMatch(dashboard, /🕒 Last updated:/);
  assert.doesNotMatch(dashboard, /Commands:/);
});

test("cashback command renders summary and claim callback", async () => {
  const subscribers = createSubscriberStore({});
  await subscribers.init();
  await subscribers.add("chat-1");
  await subscribers.setTradingWallet("chat-1", {
    publicKey: otherWallet,
    encryptedApiKey: "encrypted",
    apiKeyLast4: "abcd",
    provider: "local-solana",
    kind: "local-solana",
    label: null,
    createdAt: "now",
    updatedAt: "now"
  });

  const replies = [];
  const updates = [
    {
      update_id: 1,
      message: {
        text: "/cashback",
        chat: { id: "chat-1", type: "private" }
      }
    },
    {
      update_id: 2,
      callback_query: {
        id: "cashback-set-wallet",
        data: "cashback:set_payout_wallet",
        message: {
          chat: { id: "chat-1", type: "private" }
        }
      }
    },
    {
      update_id: 3,
      message: {
        text: wallet,
        chat: { id: "chat-1", type: "private" }
      }
    },
    {
      update_id: 4,
      callback_query: {
        id: "cashback-claim",
        data: "cashback:claim",
        message: {
          chat: { id: "chat-1", type: "private" }
        }
      }
    }
  ];
  let updateIndex = 0;
  let poller;
  const originalFetch = globalThis.fetch;

  globalThis.fetch = async (url, init) => {
    const href = String(url);

    if (href.includes("/getMe")) {
      return new Response(JSON.stringify({ ok: true, result: { username: "copybot" } }), { status: 200 });
    }

    if (href.includes("/deleteWebhook") || href.includes("/setMyCommands") || href.includes("/answerCallbackQuery")) {
      return new Response(JSON.stringify({ ok: true, result: true }), { status: 200 });
    }

    if (href.includes("/getUpdates")) {
      if (updateIndex >= updates.length) {
        poller?.stop();
        return new Response(JSON.stringify({ ok: true, result: [] }), { status: 200 });
      }

      return new Response(JSON.stringify({ ok: true, result: [updates[updateIndex++]] }), { status: 200 });
    }

    if (href.includes("/sendMessage")) {
      replies.push(JSON.parse(String(init?.body)));
      return new Response(JSON.stringify({ ok: true, result: {} }), { status: 200 });
    }

    throw new Error(`Unexpected Telegram request: ${href}`);
  };

  try {
    poller = createTelegramCommandPoller({
      config: {
        ...config,
        telegramToken: "token"
      },
      testMessage: () => "",
      subscribers,
      cashback: {
        async getSummary({ tradingWalletPublicKey }) {
          const payoutWalletPublicKey = subscribers.get("chat-1")?.cashbackPayoutWalletAddress || null;
          return {
            enabled: true,
            tradingWalletPublicKey,
            payoutWalletPublicKey,
            accruedLamports: 10_000_000n,
            claimableLamports: 10_000_000n,
            pendingLamports: 0n,
            lifetimePaidLamports: 0n,
            minClaimLamports: 5_000_000n,
            payoutUnavailableReason: payoutWalletPublicKey ? null : "add a payout wallet"
          };
        },
        async claim({ tradingWalletPublicKey }) {
          const payoutWalletPublicKey = subscribers.get("chat-1")?.cashbackPayoutWalletAddress || null;
          return {
            ok: true,
            status: "submitted",
            signature: "cashback-signature",
            errorText: null,
            summary: {
              enabled: true,
              tradingWalletPublicKey,
              payoutWalletPublicKey,
              accruedLamports: 10_000_000n,
              claimableLamports: 0n,
              pendingLamports: 0n,
              lifetimePaidLamports: 10_000_000n,
              minClaimLamports: 5_000_000n,
              payoutUnavailableReason: null
            }
          };
        }
      }
    });
    await poller.start();
  } finally {
    globalThis.fetch = originalFetch;
  }

  assert.match(replies[0].text, /<b>💎 Cashback Vault<\/b>/);
  assert.match(replies[0].text, /Claimable: <b>0\.01 SOL<\/b>/);
  assert.match(replies[0].text, /Lifetime Paid: 0 SOL/);
  assert.doesNotMatch(replies[0].text, /Accrued:/);
  assert.doesNotMatch(replies[0].text, /Pending:/);
  assert.deepEqual(
    replies[0].reply_markup.inline_keyboard.some((row) =>
      row.some((button) => button.text === "Add Payout Wallet" && button.callback_data === "cashback:set_payout_wallet")
    ),
    true
  );
  assert.match(replies[1].text, /Set cashback payout wallet/);
  assert.match(replies[2].text, /Cashback payout wallet saved/);
  assert.match(replies[2].text, new RegExp(wallet));
  assert.deepEqual(
    replies[2].reply_markup.inline_keyboard.some((row) =>
      row.some((button) => button.text === "Claim Cashback" && button.callback_data === "cashback:claim")
    ),
    true
  );
  assert.match(replies[3].text, /Cashback submitted/);
  assert.match(replies[3].text, /cashback-signature/);
  assert.deepEqual(
    replies[3].reply_markup.inline_keyboard.some((row) =>
      row.some((button) => button.text === "Claim Cashback")
    ),
    false
  );
});

test("alert mode toggles individual token alert types", () => {
  assert.equal(toggleAlertMode("both", "migrations"), "newtokens");
  assert.equal(toggleAlertMode("both", "newtokens"), "migrations");
  assert.equal(toggleAlertMode("migrations", "migrations"), null);
  assert.equal(toggleAlertMode("newtokens", "migrations"), "both");
  assert.equal(toggleAlertMode(null, "migrations"), "migrations");
  assert.equal(toggleAlertMode(null, "newtokens"), "newtokens");
});

test("copytrade execution setting inputs parse slippage and priority fees", () => {
  assert.equal(parseSlippageInput("10"), 10);
  assert.equal(parseSlippageInput("10%"), 10);
  assert.equal(parseSlippageInput("2.5"), 2.5);
  assert.equal(parseSlippageInput("0"), null);
  assert.equal(parseSlippageInput("0.09"), null);
  assert.equal(parseSlippageInput("100.1"), null);
  assert.equal(parseSlippageInput("abc"), null);

  assert.equal(parsePriorityFeeInput("0.00005"), 0.00005);
  assert.equal(parsePriorityFeeInput("1"), 1);
  assert.equal(parsePriorityFeeInput("0"), null);
  assert.equal(parsePriorityFeeInput("1.1"), null);
  assert.equal(parsePriorityFeeInput("abc"), null);
});

test("trailing sell inputs parse custom steps and formula presets", () => {
  assert.deepEqual(parseTrailingSellStepInput("20% 10s"), {
    percent: 20,
    delayMs: 10_000
  });
  assert.deepEqual(parseTrailingSellStepInput("50 after 2m"), {
    percent: 50,
    delayMs: 120_000
  });
  assert.deepEqual(parseTrailingSellStepInput("100% 1h"), {
    percent: 100,
    delayMs: 3_600_000
  });
  assert.equal(parseTrailingSellStepInput("101% 10s"), null);
  assert.equal(parseTrailingSellStepInput("20% forever"), null);
  assert.deepEqual(parseTrailingSellStepsInput("30% 2m, 20% 10s"), [
    { percent: 20, delayMs: 10_000 },
    { percent: 30, delayMs: 120_000 }
  ]);
  assert.deepEqual(parseTrailingSellStepsInput("50% 1s, 100%, 2s"), [
    { percent: 50, delayMs: 1_000 },
    { percent: 100, delayMs: 2_000 }
  ]);
  assert.deepEqual(parseTrailingSellFormulaInput("20% 10s 20% 30s 2m"), [
    { percent: 20, delayMs: 10_000 },
    { percent: 20, delayMs: 40_000 },
    { percent: 20, delayMs: 70_000 },
    { percent: 20, delayMs: 100_000 },
    { percent: 100, delayMs: 120_000 }
  ]);
  assert.equal(parseTrailingSellFormulaInput("20% 10s 20% 0s 2m"), null);
  assert.equal(parseTrailingSellFormulaInput("20% 10s 20% 30s 5s"), null);
});

test("copytrade dashboard text uses clean Bloom-style status card", () => {
  const dashboard = formatCopyTradeDashboardText({
    tradingWalletPublicKey: otherWallet,
    copyAmountSol: 0.5,
    copyTradeWallets: [
      {
        address: wallet,
        label: "cented",
        addedAt: "2026-05-23T00:00:00.000Z",
        updatedAt: "2026-05-23T00:00:00.000Z"
      }
    ]
  });

  assert.match(dashboard, /🔎 Copy Trading/);
  assert.match(dashboard, /Automatically mirror trades from selected wallets in real time\./);
  assert.match(dashboard, /👛 Trading Wallet:/);
  assert.match(dashboard, /62qc2C\.\.\.fafNgV/);
  assert.match(dashboard, /⚙️ Copy Settings<\/b>\n├ Copy Amount: 0.5 SOL/);
  assert.match(dashboard, /├ Buy: 10% slip \/ 0.00005 SOL priority/);
  assert.match(dashboard, /├ Sell: 10% slip \/ 0.00005 SOL priority/);
  assert.match(dashboard, /└ Retry Failed Buys: Off/);
  assert.match(dashboard, /🎯 Copytrade Wallets:<\/b> 1/);
  assert.match(dashboard, /└ cented/);
  assert.doesNotMatch(dashboard, new RegExp(wallet));
  assert.match(dashboard, /🟢 Setup is <b>active<\/b>/);
  assert.match(dashboard, /📉 Trailing Sells:<\/b> Not configured/);
  assert.doesNotMatch(dashboard, /🕒 Last updated:/);

  const missing = formatCopyTradeDashboardText({
    tradingWalletPublicKey: null,
    copyAmountSol: null,
    copyTradeWallets: [
      {
        address: wallet,
        label: null,
        addedAt: "2026-05-23T00:00:00.000Z",
        updatedAt: "2026-05-23T00:00:00.000Z"
      }
    ],
    buySlippagePercent: 12.5,
    buyPriorityFeeSol: 0.00012,
    sellSlippagePercent: 20,
    sellPriorityFeeSol: 0.0002,
    retryFailedCopyBuys: true
  });

  assert.match(missing, /└ 39azUY\.\.\.5jUJjg/);
  assert.match(missing, /├ Copy Amount: Not set/);
  assert.match(missing, /├ Buy: 12.5% slip \/ 0.00012 SOL priority/);
  assert.match(missing, /├ Sell: 20% slip \/ 0.0002 SOL priority/);
  assert.match(missing, /└ Retry Failed Buys: On/);
  assert.match(missing, /🔴 Setup is <b>inactive<\/b>/);
});

test("copytrade settings callback toggles retry failed buys and reset clears it", async () => {
  const subscribers = createSubscriberStore({});
  await subscribers.init();
  await subscribers.add("chat-1");

  async function runCallback(data) {
    const originalFetch = globalThis.fetch;
    const replies = [];
    let updateServed = false;
    let poller;

    globalThis.fetch = async (url, init) => {
      const href = String(url);

      if (href.includes("/getMe")) {
        return new Response(JSON.stringify({ ok: true, result: { username: "copybot" } }), { status: 200 });
      }

      if (href.includes("/deleteWebhook") || href.includes("/setMyCommands") || href.includes("/answerCallbackQuery")) {
        return new Response(JSON.stringify({ ok: true, result: true }), { status: 200 });
      }

      if (href.includes("/getUpdates")) {
        if (updateServed) {
          poller?.stop();
          return new Response(JSON.stringify({ ok: true, result: [] }), { status: 200 });
        }

        updateServed = true;
        return new Response(
          JSON.stringify({
            ok: true,
            result: [
              {
                update_id: 1,
                callback_query: {
                  id: "callback-1",
                  data,
                  message: {
                    chat: {
                      id: "chat-1",
                      type: "private"
                    }
                  }
                }
              }
            ]
          }),
          { status: 200 }
        );
      }

      if (href.includes("/sendMessage")) {
        replies.push(JSON.parse(String(init?.body)));
        poller?.stop();
        return new Response(JSON.stringify({ ok: true, result: {} }), { status: 200 });
      }

      throw new Error(`Unexpected Telegram request: ${href}`);
    };

    try {
      poller = createTelegramCommandPoller({
        config: {
          ...config,
          telegramToken: "token"
        },
        testMessage: () => "",
        subscribers
      });
      await poller.start();
    } finally {
      globalThis.fetch = originalFetch;
    }

    return replies;
  }

  const toggleReplies = await runCallback("copytrade:settings:retry_failed_buys");
  assert.equal(subscribers.get("chat-1")?.copyTradeRetryFailedBuys, true);
  assert.match(toggleReplies[0].text, /Retry failed copy buys is now on/);
  assert.deepEqual(
    toggleReplies[0].reply_markup.inline_keyboard.some((row) =>
      row.some((button) => button.text === "☑ Retry Failed Buys")
    ),
    true
  );

  const resetReplies = await runCallback("copytrade:settings:reset");
  assert.equal(subscribers.get("chat-1")?.copyTradeRetryFailedBuys, false);
  assert.equal(subscribers.get("chat-1")?.copyTradeBuyPressureSellEnabled, false);
  assert.equal(subscribers.get("chat-1")?.copyTradeBuyPressureSellTimeoutMs, null);
  assert.match(resetReplies[0].text, /Execution settings reset to inherited defaults/);
  assert.deepEqual(
    resetReplies[0].reply_markup.inline_keyboard.some((row) =>
      row.some((button) => button.text === "☐ Retry Failed Buys")
    ),
    true
  );
});

test("copytrade settings callback toggles buy-pressure sells and saves timeout", async () => {
  const subscribers = createSubscriberStore({});
  await subscribers.init();
  await subscribers.add("chat-1");

  async function runUpdates(updates, configOverrides = {}) {
    const originalFetch = globalThis.fetch;
    const replies = [];
    let updateIndex = 0;
    let poller;

    globalThis.fetch = async (url, init) => {
      const href = String(url);

      if (href.includes("/getMe")) {
        return new Response(JSON.stringify({ ok: true, result: { username: "copybot" } }), { status: 200 });
      }

      if (href.includes("/deleteWebhook") || href.includes("/setMyCommands") || href.includes("/answerCallbackQuery")) {
        return new Response(JSON.stringify({ ok: true, result: true }), { status: 200 });
      }

      if (href.includes("/getUpdates")) {
        if (updateIndex >= updates.length) {
          poller?.stop();
          return new Response(JSON.stringify({ ok: true, result: [] }), { status: 200 });
        }

        const update = updates[updateIndex++];
        return new Response(JSON.stringify({ ok: true, result: [update] }), { status: 200 });
      }

      if (href.includes("/sendMessage")) {
        replies.push(JSON.parse(String(init?.body)));
        return new Response(JSON.stringify({ ok: true, result: {} }), { status: 200 });
      }

      throw new Error(`Unexpected Telegram request: ${href}`);
    };

    try {
      poller = createTelegramCommandPoller({
        config: {
          ...config,
          copyTradeBuyPressureSellEnabled: true,
          copyTradeBuyPressureSellTimeoutMs: 5000,
          telegramToken: "token",
          ...configOverrides
        },
        testMessage: () => "",
        subscribers
      });
      await poller.start();
    } finally {
      globalThis.fetch = originalFetch;
    }

    return replies;
  }

  const callbackUpdate = (data, updateId = 1) => ({
    update_id: updateId,
    callback_query: {
      id: `callback-${updateId}`,
      data,
      message: {
        chat: {
          id: "chat-1",
          type: "private"
        }
      }
    }
  });
  const messageUpdate = (text, updateId = 1) => ({
    update_id: updateId,
    message: {
      text,
      chat: {
        id: "chat-1",
        type: "private"
      }
    }
  });

  const toggleReplies = await runUpdates([callbackUpdate("copytrade:settings:buy_pressure_toggle")]);
  assert.equal(subscribers.get("chat-1")?.copyTradeBuyPressureSellEnabled, true);
  assert.match(toggleReplies.at(-1).text, /Buy-pressure sell is now on/);
  assert.match(toggleReplies.at(-1).text, /<b>📈 Buy-Pressure Sell<\/b>\n├ Status: On/);
  assert.match(toggleReplies.at(-1).text, /├ Status: On/);
  assert.match(toggleReplies.at(-1).text, /├ Timeout: Inherited 5s/);
  assert.match(toggleReplies.at(-1).text, /└ Bot gate: Enabled/);
  assert.deepEqual(
    toggleReplies.at(-1).reply_markup.inline_keyboard.some((row) =>
      row.some((button) => button.text === "☑ Enabled")
    ),
    true
  );

  const settingsReplies = await runUpdates([callbackUpdate("copytrade:settings", 5)]);
  assert.deepEqual(
    settingsReplies.at(-1).reply_markup.inline_keyboard.some((row) =>
      row.some((button) => button.text === "☑ Buy-Pressure Sell · 5s")
    ),
    true
  );

  const timeoutReplies = await runUpdates([
    callbackUpdate("copytrade:settings:buy_pressure_timeout", 10),
    messageUpdate("5", 11),
    callbackUpdate("copytrade:confirm_pending", 12)
  ]);
  assert.equal(subscribers.get("chat-1")?.copyTradeBuyPressureSellTimeoutMs, 5000);
  assert.match(timeoutReplies.at(-1).text, /Buy-pressure timeout saved:<\/b> 5s/);
  assert.match(timeoutReplies.at(-1).text, /├ Timeout: Custom 5s/);

  const resetReplies = await runUpdates([callbackUpdate("copytrade:settings:reset", 20)]);
  assert.equal(subscribers.get("chat-1")?.copyTradeBuyPressureSellEnabled, false);
  assert.equal(subscribers.get("chat-1")?.copyTradeBuyPressureSellTimeoutMs, null);
  assert.match(resetReplies.at(-1).text, /Execution settings reset to inherited defaults/);
  assert.match(resetReplies.at(-1).text, /├ Status: Off/);
  assert.match(resetReplies.at(-1).text, /├ Timeout: Inherited 5s/);
  assert.match(resetReplies.at(-1).text, /└ Bot gate: Enabled/);
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
    assert.equal(
      await store.setTradingWallet("chat-1", {
        publicKey: otherWallet,
        encryptedApiKey: encryptSecret("pump-key-alpha", encryptionSecret),
        apiKeyLast4: "lpha",
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
          { percent: 25, delayMs: 15_000 },
          { percent: 100, delayMs: 60_000 }
        ],
        updatedAt: "2026-05-23T01:00:00.000Z"
      }),
      true
    );
    assert.equal(await store.setCopyTradeWalletTrailingSellConfig("chat-1", "missing", null), false);
    assert.equal(await store.renameCopyTradeWallet("chat-1", wallet, "Copy Gamma"), true);
    assert.equal(await store.renameCopyTradeWallet("chat-1", wallet, null), true);
    assert.equal(await store.renameCopyTradeWallet("chat-1", "missing", "Nope"), false);
    assert.equal(await store.watchCopyTradeWallet("chat-2", wallet, "Unverified"), false);
    assert.equal(await store.setCopyWallet("chat-2", otherWallet), false);
    assert.equal(await store.setCopyAmountSol("chat-2", 0.25), false);
    assert.equal(await store.setCopyTradeBuySlippage("chat-1", 12.5), true);
    assert.equal(await store.setCopyTradeBuyPriorityFee("chat-1", 0.00012), true);
    assert.equal(await store.setCopyTradeSellSlippage("chat-1", 20), true);
    assert.equal(await store.setCopyTradeSellPriorityFee("chat-1", 0.0002), true);
    assert.equal(await store.setCopyTradeRetryFailedBuys("chat-1", true), true);
    assert.equal(await store.setCopyTradeBuyPressureSellEnabled("chat-1", true), true);
    assert.equal(await store.setCopyTradeBuyPressureSellTimeoutMs("chat-1", 5000), true);
    assert.equal(await store.setCopyTradeBuySlippage("chat-2", 12.5), false);
    assert.equal(await store.setCopyTradeSellPriorityFee("chat-2", 0.0002), false);
    assert.equal(await store.setCopyTradeRetryFailedBuys("chat-2", true), false);
    assert.equal(await store.setCopyTradeBuyPressureSellEnabled("chat-2", true), false);
    assert.equal(await store.setCopyTradeBuyPressureSellTimeoutMs("chat-2", 5000), false);
    assert.equal(store.get("chat-1")?.copyWalletAddress, otherWallet);
    assert.deepEqual(store.get("chat-1")?.copyWalletAddresses, [otherWallet, wallet]);
    assert.deepEqual(store.listCopyWallets("chat-1"), [otherWallet, wallet]);
    assert.equal(store.get("chat-1")?.copyAmountSol, 0.25);
    assert.equal(store.get("chat-1")?.copyTradeBuySlippagePercent, 12.5);
    assert.equal(store.get("chat-1")?.copyTradeBuyPriorityFeeSol, 0.00012);
    assert.equal(store.get("chat-1")?.copyTradeSellSlippagePercent, 20);
    assert.equal(store.get("chat-1")?.copyTradeSellPriorityFeeSol, 0.0002);
    assert.equal(store.get("chat-1")?.copyTradeRetryFailedBuys, true);
    assert.equal(store.get("chat-1")?.copyTradeBuyPressureSellEnabled, true);
    assert.equal(store.get("chat-1")?.copyTradeBuyPressureSellTimeoutMs, 5000);
    assert.equal(store.getTradingWallet("chat-1")?.publicKey, otherWallet);
    assert.equal(store.getTradingWallet("chat-1")?.label, "Main Wallet");
    assert.equal(decryptSecret(store.getTradingWallet("chat-1")?.encryptedApiKey || "", encryptionSecret), "pump-key-alpha");
    assert.deepEqual(
      store.listCopyTradeWallets("chat-1").map((entry) => [entry.address, entry.label, entry.trailingSellConfig?.percentBasis || null]),
      [
        [wallet, null, null],
        [otherWallet, "Copy Alpha", "original_position"]
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
    assert.equal(reloaded.get("chat-1")?.copyAmountSol, 0.25);
    assert.equal(reloaded.get("chat-1")?.copyTradeBuySlippagePercent, 12.5);
    assert.equal(reloaded.get("chat-1")?.copyTradeBuyPriorityFeeSol, 0.00012);
    assert.equal(reloaded.get("chat-1")?.copyTradeSellSlippagePercent, 20);
    assert.equal(reloaded.get("chat-1")?.copyTradeSellPriorityFeeSol, 0.0002);
    assert.equal(reloaded.get("chat-1")?.copyTradeRetryFailedBuys, true);
    assert.equal(reloaded.get("chat-1")?.copyTradeBuyPressureSellEnabled, true);
    assert.equal(reloaded.get("chat-1")?.copyTradeBuyPressureSellTimeoutMs, 5000);
    assert.equal(reloaded.getTradingWallet("chat-1")?.publicKey, otherWallet);
    assert.equal(reloaded.getTradingWallet("chat-1")?.label, "Main Wallet");
    assert.equal(await reloaded.renameTradingWallet("chat-1", null), true);
    assert.equal(reloaded.getTradingWallet("chat-1")?.label, null);
    assert.deepEqual(
      reloaded.listCopyTradeWallets("chat-1").map((entry) => [entry.address, entry.label, entry.trailingSellConfig?.steps.length || 0]),
      [
        [wallet, null, 0],
        [otherWallet, "Copy Alpha", 2]
      ]
    );
    assert.equal(reloaded.get("chat-1")?.mode, null);
    assert.equal(await reloaded.resetCopyTradeExecutionSettings("chat-1"), true);
    assert.equal(reloaded.get("chat-1")?.copyTradeBuySlippagePercent, null);
    assert.equal(reloaded.get("chat-1")?.copyTradeBuyPriorityFeeSol, null);
    assert.equal(reloaded.get("chat-1")?.copyTradeSellSlippagePercent, null);
    assert.equal(reloaded.get("chat-1")?.copyTradeSellPriorityFeeSol, null);
    assert.equal(reloaded.get("chat-1")?.copyTradeRetryFailedBuys, false);
    assert.equal(reloaded.get("chat-1")?.copyTradeBuyPressureSellEnabled, false);
    assert.equal(reloaded.get("chat-1")?.copyTradeBuyPressureSellTimeoutMs, null);
    assert.equal(await reloaded.resetCopyTradeExecutionSettings("chat-2"), false);

    assert.equal(await reloaded.unwatchWallet("chat-1", wallet), true);
    assert.deepEqual(reloaded.listWatchedWallets("chat-1"), []);
    assert.deepEqual(
      reloaded.listCopyTradeWallets("chat-1").map((entry) => entry.address),
      [wallet, otherWallet]
    );
    assert.equal(await reloaded.unwatchCopyTradeWallet("chat-1", wallet), true);
    assert.deepEqual(reloaded.listCopyTradeWallets("chat-1").map((entry) => entry.address), [otherWallet]);
    assert.equal(await reloaded.unwatchAllCopyTradeWallets("chat-1"), 1);
    assert.deepEqual(reloaded.listCopyTradeWallets("chat-1"), []);
    assert.equal(reloaded.getTradingWallet("chat-1")?.publicKey, otherWallet);
    assert.equal(await reloaded.unwatchAllCopyTradeWallets("chat-1"), 0);

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
    events: {
      swap: {
        nativeInput: {
          account: wallet,
          amount: "125000000"
        },
        nativeOutput: null,
        tokenInputs: [],
        tokenOutputs: [
          {
            userAccount: wallet,
            mint,
            rawTokenAmount: {
              tokenAmount: "250000000000",
              decimals: 6
            },
            symbol: "BONK"
          }
        ]
      }
    },
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
  assert.match(message, /👀 Wallet Trade/);
  assert.match(message, /🟢 Buy detected/);
  assert.match(message, /Alpha &lt;Wallet&gt;/);
  assert.match(message, /0.125 SOL -> 250,000 BONK/);
  assert.match(message, /0.125 SOL/);
  assert.match(message, /<b>📡 Route<\/b>/);
  assert.match(message, /└ Source: JUPITER/);

  const copyMessage = formatWalletTradeMessageWithCopySettings(trade, {
    copyWalletAddress: otherWallet,
    copyWalletAddresses: [otherWallet, wallet],
    copyAmountSol: 0.25
  });
  assert.match(copyMessage, /⚡ Copy Trade Setup/);
  assert.match(copyMessage, new RegExp(otherWallet));
  assert.match(copyMessage, new RegExp(wallet));
  assert.match(copyMessage, /├ Copy Wallets: 2/);
  assert.match(copyMessage, /└ Copy Amount: 0.25 SOL/);
  assert.match(copyMessage, /🟢 Ready to copy <b>0.25 SOL<\/b> into this token from 2 wallet/);

  assert.equal(isCopyableSolToTokenBuy(trade), true);
  assert.equal(
    isCopyableSolToTokenBuy({
      ...trade,
      input: {
        mint: "So11111111111111111111111111111111111111112",
        symbol: null,
        amount: 4.889866666
      },
      output: {
        mint,
        symbol: null,
        amount: 137035949.044437
      }
    }),
    true
  );
  const simulationMessage = formatCopyTradeSimulationMessage(trade, {
    copyWalletAddress: otherWallet,
    copyAmountSol: 0.25
  });
  assert.match(simulationMessage || "", /⚡ Copy Trade Simulation/);
  assert.match(simulationMessage || "", /🟡 Would buy this token/);
  assert.match(simulationMessage || "", /Alpha &lt;Wallet&gt;/);
  assert.match(simulationMessage || "", new RegExp(otherWallet));
  assert.match(simulationMessage || "", new RegExp(mint));
  assert.match(simulationMessage || "", /<b>🧱 Build<\/b>/);
  assert.match(simulationMessage || "", /└ Local transaction build not requested/);
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
  assert.match(builtSimulationMessage || "", /└ Local transaction built \(1,234 bytes\)/);

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
  assert.match(autoBuyMessage || "", /⚡ Auto Copy Buy/);
  assert.match(autoBuyMessage || "", /🟢 Buy submitted/);
  assert.match(autoBuyMessage || "", /🎯 Copy Buy/);
  assert.match(autoBuyMessage || "", /├ Target: Alpha/);
  assert.match(autoBuyMessage || "", /├ Trading Wallet:/);
  assert.match(autoBuyMessage || "", /└ Copy Amount: 0.25 SOL/);
  assert.match(autoBuyMessage || "", /🪙 Contract Address/);
  assert.match(autoBuyMessage || "", /└ Tx: <code>tx-alpha<\/code>/);
  assert.doesNotMatch(autoBuyMessage || "", /PumpPortal:/);
  assert.match(autoBuyMessage || "", new RegExp(otherWallet));

  const nicknamedAutoBuyMessage = formatAutoCopyBuyMessage({
    trade: {
      ...trade,
      label: "cented"
    },
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
  assert.match(nicknamedAutoBuyMessage || "", /├ Target: cented/);
  assert.doesNotMatch(nicknamedAutoBuyMessage || "", new RegExp(wallet));

  const failedAutoBuyMessage = formatAutoCopyBuyMessage({
    trade,
    tradingWalletPublicKey: otherWallet,
    copyAmountSol: 0.25,
    result: {
      ok: false,
      status: 429,
      signature: null,
      errorText: "rate limited",
      raw: "rate limited"
    }
  });
  assert.match(failedAutoBuyMessage || "", /⚡ Auto Copy Buy/);
  assert.match(failedAutoBuyMessage || "", /🔴 Buy failed/);
  assert.match(failedAutoBuyMessage || "", /└ Failed: HTTP 429 - rate limited/);
  assert.doesNotMatch(failedAutoBuyMessage || "", /PumpPortal:/);

  const sellRequest = buildPumpPortalLightningSellRequest({
    mint,
    amountPercent: 20,
    slippage: 15,
    priorityFee: 0.00009,
    pool: "auto"
  });

  assert.deepEqual(sellRequest, {
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
  assert.match(trailingScheduledMessage || "", /📉 Trailing Sells/);
  assert.match(trailingScheduledMessage || "", /🟢 Sell schedule created/);
  assert.match(trailingScheduledMessage || "", /Sell 20% after 2s/);
  assert.match(trailingScheduledMessage || "", /Sell 100% after 4s/);
  assert.doesNotMatch(trailingScheduledMessage || "", /Build-only/);

  const trailingSkippedMessage = formatCopyTradeTrailingSellSkippedMessage({
    trade,
    reason: "copy buy was not confirmed before trailing sell scheduling timeout"
  });
  assert.match(trailingSkippedMessage || "", /📉 Trailing Sells/);
  assert.match(trailingSkippedMessage || "", /🟡 Sell schedule skipped/);
  assert.match(trailingSkippedMessage || "", /not confirmed/);

  const trailingResultMessage = formatCopyTradeTrailingSellResultMessage({
    trade,
    stepIndex: 1,
    totalSteps: 2,
    request: {
      ...sellRequest,
      amount: "100%"
    },
    result: {
      ok: true,
      status: 200,
      signature: "sell-tx-beta",
      errorText: null,
      raw: { signature: "sell-tx-beta" }
    }
  });
  assert.match(trailingResultMessage, /📉 Trailing Sell/);
  assert.match(trailingResultMessage, /🟢 Sell submitted/);
  assert.match(trailingResultMessage, /├ Step: 2\/2/);
  assert.match(trailingResultMessage, /└ Sell Amount: 100%/);
  assert.match(trailingResultMessage, /🪙 Contract Address/);
  assert.match(trailingResultMessage, /└ Tx: <code>sell-tx-beta<\/code>/);

  const duplicateTrailingMessage = formatCopyTradeTrailingSellResultMessage({
    trade,
    stepIndex: 1,
    totalSteps: 2,
    request: {
      ...sellRequest,
      amount: "20%"
    },
    result: {
      ok: true,
      status: 200,
      signature: "sell-tx-beta",
      errorText: null,
      raw: { signature: "sell-tx-beta" }
    },
    duplicateSignature: true
  });
  assert.match(duplicateTrailingMessage, /🟡 Duplicate tx returned/);
  assert.match(duplicateTrailingMessage, /already used earlier/);

  const trailingFailureMessage = formatCopyTradeTrailingSellResultMessage({
    trade,
    stepIndex: 0,
    totalSteps: 2,
    request: sellRequest,
    result: {
      ok: false,
      status: 500,
      signature: null,
      errorText: "sell failed",
      raw: "sell failed"
    }
  });
  assert.match(trailingFailureMessage, /🔴 Sell failed/);
  assert.match(trailingFailureMessage, /└ Failed: HTTP 500 - sell failed/);
  assert.doesNotMatch(trailingFailureMessage, /PumpPortal:/);
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
      ],
      events: {
        swap: {
          nativeInput: null,
          nativeOutput: {
            account: wallet,
            amount: "1500000000"
          },
          tokenInputs: [
            {
              userAccount: wallet,
              mint,
              rawTokenAmount: {
                tokenAmount: "100000000",
                decimals: 6
              },
              symbol: "BONK"
            }
          ],
          tokenOutputs: []
        }
      }
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
      ],
      events: {
        swap: {
          nativeInput: null,
          nativeOutput: null,
          tokenInputs: [
            {
              userAccount: wallet,
              mint,
              rawTokenAmount: {
                tokenAmount: "100000000",
                decimals: 6
              },
              symbol: "BONK"
            }
          ],
          tokenOutputs: [
            {
              userAccount: wallet,
              mint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY6JqTd6FJqyf2s",
              rawTokenAmount: {
                tokenAmount: "12500000",
                decimals: 6
              },
              symbol: "USDT"
            }
          ]
        }
      }
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
    isCopyableSolToTokenBuy({
      ...tokenToSol,
      input: {
        mint,
        symbol: null,
        amount: 100
      },
      output: {
        mint: "So11111111111111111111111111111111111111112",
        symbol: null,
        amount: 1.5
      }
    }),
    false
  );
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
    /⚪ Not a copyable SOL-to-token buy/
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

test("Helius webhook receiver waits for event processing before ACKing success", async () => {
  let releaseProcessing;
  let responseSettled = false;
  let processed = false;
  let markProcessingStarted;
  const processingStarted = new Promise((resolve) => {
    markProcessingStarted = resolve;
  });
  const server = createHeliusWebhookServer({
    authHeader: "Bearer secret",
    port: 0,
    onEvents: async () => {
      markProcessingStarted();
      await new Promise((resolve) => {
        releaseProcessing = resolve;
      });
      processed = true;
    }
  });

  await server.start();
  const port = server.port();
  assert.ok(port);

  try {
    const responsePromise = fetch(`http://127.0.0.1:${port}/webhooks/helius`, {
      method: "POST",
      headers: {
        authorization: "Bearer secret",
        "content-type": "application/json"
      },
      body: JSON.stringify([heliusSwapEvent()])
    }).then(async (response) => ({
      body: await response.json(),
      status: response.status
    }));
    responsePromise.then(
      () => {
        responseSettled = true;
      },
      () => {
        responseSettled = true;
      }
    );

    await processingStarted;
    await new Promise((resolve) => setTimeout(resolve, 10));
    assert.equal(responseSettled, false);

    releaseProcessing();
    const response = await responsePromise;

    assert.equal(processed, true);
    assert.equal(response.status, 200);
    assert.deepEqual(response.body, { ok: true, received: 1 });
  } finally {
    await server.stop();
  }
});

test("Helius webhook receiver returns non-2xx when event processing fails", async () => {
  const originalConsoleError = console.error;
  console.error = () => {};
  const server = createHeliusWebhookServer({
    authHeader: "Bearer secret",
    port: 0,
    onEvents: async () => {
      throw new Error("processor failed");
    }
  });

  await server.start();
  const port = server.port();
  assert.ok(port);

  try {
    const response = await fetch(`http://127.0.0.1:${port}/webhooks/helius`, {
      method: "POST",
      headers: {
        authorization: "Bearer secret",
        "content-type": "application/json"
      },
      body: JSON.stringify([heliusSwapEvent()])
    });

    assert.equal(response.status, 500);
    assert.deepEqual(await response.json(), { ok: false, error: "event processing failed" });
  } finally {
    await server.stop();
    console.error = originalConsoleError;
  }
});
