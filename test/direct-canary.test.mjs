import assert from "node:assert/strict";
import test from "node:test";
import { directCanaryBlockedReason } from "../dist/direct-canary.js";

const liveDirectConfig = {
  directExecutionEnabled: true,
  directExecutionLiveEnabled: true,
  directExecutionBuildOnly: false,
  directExecutionSimulateOnly: false,
  directExecutionCanaryChatIds: ["123"],
  directExecutionCanaryWallets: []
};

test("direct canary gate ignores non-direct providers", () => {
  assert.equal(
    directCanaryBlockedReason({
      provider: "pumpportal-lightning",
      chatId: "999",
      tradingWalletPublicKey: "WalletA",
      config: { ...liveDirectConfig, directExecutionCanaryChatIds: [] }
    }),
    null
  );
});

test("direct canary gate allows every chat when direct mode is active", () => {
  assert.equal(
    directCanaryBlockedReason({
      provider: "direct-auto",
      chatId: "999",
      tradingWalletPublicKey: "WalletA",
      config: {
        ...liveDirectConfig,
        directExecutionCanaryChatIds: [],
        directExecutionCanaryWallets: []
      }
    }),
    null
  );
});

test("direct canary gate ignores configured chat canaries", () => {
  assert.equal(
    directCanaryBlockedReason({
      provider: "direct-auto",
      chatId: "999",
      tradingWalletPublicKey: "WalletA",
      config: liveDirectConfig
    }),
    null
  );
});

test("direct canary gate still enforces configured wallet canaries", () => {
  assert.equal(
    directCanaryBlockedReason({
      provider: "direct-auto",
      chatId: "999",
      tradingWalletPublicKey: "WalletA",
      config: {
        ...liveDirectConfig,
        directExecutionCanaryWallets: ["WalletB"]
      }
    }),
    "trading wallet WalletA is not in DIRECT_EXECUTION_CANARY_WALLETS"
  );

  assert.equal(
    directCanaryBlockedReason({
      provider: "direct-auto",
      chatId: "999",
      tradingWalletPublicKey: "WalletB",
      config: {
        ...liveDirectConfig,
        directExecutionCanaryWallets: ["WalletB"]
      }
    }),
    null
  );
});
