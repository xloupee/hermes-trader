import assert from "node:assert/strict";
import test from "node:test";
import { copyBuySubmissionKey, createCopyBuySubmissionGuard } from "../dist/copytrade-guard.js";

test("copy buy submission guard dedupes exact observed transaction only", () => {
  const firstBuy = copyBuySubmissionKey({
    chatId: "chat-1",
    tradingWalletPublicKey: "TradingWallet1111111111111111111111111111111",
    sourceWalletAddress: "SourceWallet11111111111111111111111111111111",
    observedSignature: "target-buy-signature-1"
  });
  const secondBuySameMintDifferentSignature = copyBuySubmissionKey({
    chatId: "chat-1",
    tradingWalletPublicKey: "TradingWallet1111111111111111111111111111111",
    sourceWalletAddress: "SourceWallet11111111111111111111111111111111",
    observedSignature: "target-buy-signature-2"
  });
  const exactReplay = copyBuySubmissionKey({
    chatId: "chat-1",
    tradingWalletPublicKey: "TradingWallet1111111111111111111111111111111",
    sourceWalletAddress: "SourceWallet11111111111111111111111111111111",
    observedSignature: "target-buy-signature-1"
  });

  assert.notEqual(firstBuy, secondBuySameMintDifferentSignature);
  assert.equal(firstBuy, exactReplay);

  const guard = createCopyBuySubmissionGuard();
  assert.equal(guard.reserve(firstBuy), true);
  assert.equal(guard.reserve(exactReplay), false);
  assert.equal(guard.reserve(secondBuySameMintDifferentSignature), true);
  assert.equal(guard.size(), 2);

  guard.release(firstBuy);
  assert.equal(guard.reserve(exactReplay), true);
});

test("copy buy submission guard allows unkeyed submissions", () => {
  const guard = createCopyBuySubmissionGuard();

  assert.equal(copyBuySubmissionKey({
    chatId: "chat-1",
    tradingWalletPublicKey: "TradingWallet1111111111111111111111111111111",
    sourceWalletAddress: "SourceWallet11111111111111111111111111111111",
    observedSignature: null
  }), null);
  assert.equal(guard.reserve(null), true);
  assert.equal(guard.reserve(null), true);
});
