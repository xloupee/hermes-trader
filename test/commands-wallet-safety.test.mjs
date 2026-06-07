import assert from "node:assert/strict";
import test from "node:test";
import {
  canCreatePumpPortalTradingWalletInChat,
  copyTradeEmergencyResumeConfirmReplyMarkup,
  copyTradeEmergencyStopConfirmReplyMarkup,
  copyTradeRiskSettingConfirmReplyMarkup,
  formatCopyTradeEmergencyResumeActivatedText,
  formatCopyTradeEmergencyResumeConfirmText,
  formatCopyTradeEmergencyResumeUnavailableText,
  formatCopyTradeEmergencyStopActivatedText,
  formatCopyTradeEmergencyStopConfirmText,
  formatCopyTradeEmergencyStopUnavailableText,
  formatCopyTradeRiskSettingConfirmText,
  formatTradingWalletCreateConfirmText,
  tradingWalletBackupWarningText,
  tradingWalletCreationBlockedText
} from "../dist/commands.js";

test("PumpPortal trading wallet creation only allows private chats when chat type is known", () => {
  assert.equal(canCreatePumpPortalTradingWalletInChat("private"), true);
  assert.equal(canCreatePumpPortalTradingWalletInChat(undefined), true);
  assert.equal(canCreatePumpPortalTradingWalletInChat(null), true);
  assert.equal(canCreatePumpPortalTradingWalletInChat("group"), false);
  assert.equal(canCreatePumpPortalTradingWalletInChat("supergroup"), false);
  assert.equal(canCreatePumpPortalTradingWalletInChat("channel"), false);
  assert.equal(canCreatePumpPortalTradingWalletInChat("unknown"), false);

  assert.equal(tradingWalletCreationBlockedText("private"), null);
  assert.equal(tradingWalletCreationBlockedText(undefined), null);
  assert.match(tradingWalletCreationBlockedText("group") || "", /private Telegram chat/);
});

test("trading wallet creation confirmation includes hot-wallet backup warning", () => {
  const warning = tradingWalletBackupWarningText();
  assert.match(warning, /Hot-wallet\/private-key warning/);
  assert.match(warning, /├ Bot can use it for copy buys/);
  assert.match(warning, /├ Secret key is shown once/);
  assert.match(warning, /├ Back it up privately before depositing SOL/);
  assert.match(warning, /└ The bot cannot recover it later/);

  const firstWalletConfirm = formatTradingWalletCreateConfirmText();
  assert.match(firstWalletConfirm, /Create Trading Wallet\?/);
  assert.match(firstWalletConfirm, /PumpPortal trading wallet/);
  assert.match(firstWalletConfirm, /Secret key is shown once/i);

  const localWalletConfirm = formatTradingWalletCreateConfirmText({
    provider: "local-solana"
  });
  assert.match(localWalletConfirm, /Solana signing wallet/);
  assert.match(localWalletConfirm, /Secret key is shown once/i);

  const replacementConfirm = formatTradingWalletCreateConfirmText({
    existingPublicKey: "wallet<key>"
  });
  assert.match(replacementConfirm, /Create New Trading Wallet\?/);
  assert.match(replacementConfirm, /Current wallet/);
  assert.match(replacementConfirm, /wallet&lt;key&gt;/);
  assert.match(replacementConfirm, /old wallet will still exist on-chain/);
  assert.match(replacementConfirm, /Back it up privately before depositing SOL/);
});

test("copy trade risk setting confirmation warns before saving live-affecting values", () => {
  const text = formatCopyTradeRiskSettingConfirmText("buy_slippage", 12.5);
  assert.match(text, /Confirm Buy slippage/);
  assert.match(text, /12\.5%/);
  assert.match(text, /🧾 Pending Change/);
  assert.match(text, /⚠️ Live Trade Warning/);
  assert.match(text, /├ Can affect live SOL trades/);
  assert.match(text, /└ Dry-run must be off/);

  assert.deepEqual(copyTradeRiskSettingConfirmReplyMarkup(), {
    inline_keyboard: [
      [{ text: "✅ Confirm", callback_data: "copytrade:confirm_pending" }],
      [{ text: "↩️ Cancel", callback_data: "copytrade:cancel_pending" }]
    ]
  });
});

test("copy trade emergency stop confirmation is explicit and preserves setup", () => {
  const text = formatCopyTradeEmergencyStopConfirmText();
  assert.match(text, /Stop Live Copy Trading/);
  assert.match(text, /disables live copy-trade submissions/);
  assert.match(text, /will not remove Target Wallets/);
  assert.match(text, /trading wallet config/);
  assert.match(text, /token alerts/);

  assert.deepEqual(copyTradeEmergencyStopConfirmReplyMarkup(), {
    inline_keyboard: [
      [{ text: "🛑 Confirm Stop", callback_data: "copytrade:emergency_stop_confirm" }],
      [{ text: "↩️ Back", callback_data: "copytrade:dashboard" }]
    ]
  });
});

test("copy trade emergency stop state text reports disabled live submissions", () => {
  const active = formatCopyTradeEmergencyStopActivatedText("Execution state: EMERGENCY_STOPPED");
  assert.match(active, /Emergency stop active/);
  assert.match(active, /Live copy-trade submissions are disabled/);
  assert.match(active, /were left unchanged/);
  assert.match(active, /Execution state: EMERGENCY_STOPPED/);

  const unavailable = formatCopyTradeEmergencyStopUnavailableText();
  assert.match(unavailable, /Emergency stop unavailable/);
  assert.match(unavailable, /live copy trading was not changed/);
});

test("copy trade emergency resume confirmation is explicit about env gates", () => {
  const text = formatCopyTradeEmergencyResumeConfirmText();
  assert.match(text, /Resume Live Copy Trading/);
  assert.match(text, /turns live copy-trade submissions back on/);
  assert.match(text, /🧾 What stays the same/);
  assert.match(text, /Target Wallets stay saved/);
  assert.match(text, /Trading wallet and balances stay unchanged/);
  assert.match(text, /Amount, slippage, priority, and other settings stay unchanged/);

  assert.deepEqual(copyTradeEmergencyResumeConfirmReplyMarkup(), {
    inline_keyboard: [
      [{ text: "🟢 Confirm Resume", callback_data: "copytrade:emergency_resume_confirm" }],
      [{ text: "↩️ Back", callback_data: "copytrade:dashboard" }]
    ]
  });
});

test("copy trade emergency resume state text reports live gates still apply", () => {
  const active = formatCopyTradeEmergencyResumeActivatedText("Execution state: LIVE_ENABLED");
  assert.match(active, /Emergency stop cleared/);
  assert.match(active, /no longer blocking live copy-trade submissions/);
  assert.match(active, /Live trading still depends on COPY_TRADE_ENABLED/);
  assert.match(active, /Execution state: LIVE_ENABLED/);

  const unavailable = formatCopyTradeEmergencyResumeUnavailableText();
  assert.match(unavailable, /Clear emergency stop unavailable/);
  assert.match(unavailable, /live copy trading was not changed/);
});
