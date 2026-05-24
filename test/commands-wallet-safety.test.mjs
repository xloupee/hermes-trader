import assert from "node:assert/strict";
import test from "node:test";
import {
  canCreatePumpPortalTradingWalletInChat,
  copyTradeRiskSettingConfirmReplyMarkup,
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
  assert.match(warning, /Back it up somewhere private before depositing SOL/);
  assert.match(warning, /bot cannot recover it later/);

  const firstWalletConfirm = formatTradingWalletCreateConfirmText();
  assert.match(firstWalletConfirm, /Create Trading Wallet\?/);
  assert.match(firstWalletConfirm, /PumpPortal hot wallet/);
  assert.match(firstWalletConfirm, /private key is shown once/i);

  const replacementConfirm = formatTradingWalletCreateConfirmText({
    existingPublicKey: "wallet<key>"
  });
  assert.match(replacementConfirm, /Create New Trading Wallet\?/);
  assert.match(replacementConfirm, /Current wallet/);
  assert.match(replacementConfirm, /wallet&lt;key&gt;/);
  assert.match(replacementConfirm, /old wallet will still exist on-chain/);
  assert.match(replacementConfirm, /Back it up somewhere private before depositing SOL/);
});

test("copy trade risk setting confirmation warns before saving live-affecting values", () => {
  const text = formatCopyTradeRiskSettingConfirmText("buy_slippage", 12.5);
  assert.match(text, /Confirm Buy slippage/);
  assert.match(text, /12\.5%/);
  assert.match(text, /affect live SOL trades/);

  assert.deepEqual(copyTradeRiskSettingConfirmReplyMarkup(), {
    inline_keyboard: [
      [{ text: "✅ Confirm", callback_data: "copytrade:confirm_pending" }],
      [{ text: "↩️ Cancel", callback_data: "copytrade:cancel_pending" }]
    ]
  });
});
