import assert from "node:assert/strict";
import test from "node:test";
import {
  buildPlatformFeeTransferInstruction,
  calculatePlatformFeeSplit,
  formatPlatformFeeDisclosure,
  platformFeeConfigBlockedReason
} from "../dist/platform-fee.js";

test("platform fee defaults disabled and preserves the full buy budget", () => {
  const split = calculatePlatformFeeSplit({
    action: "buy",
    budgetLamports: 1_000_000_000n,
    config: {}
  });

  assert.equal(split.enabled, false);
  assert.equal(split.bps, 100);
  assert.equal(split.feeLamports, 0n);
  assert.equal(split.tradeLamports, 1_000_000_000n);
  assert.equal(buildPlatformFeeTransferInstruction({ split, fromPubkey: "Wallet" }), null);
});

test("platform fee splits a 1 percent buy budget inclusively", () => {
  const split = calculatePlatformFeeSplit({
    action: "buy",
    budgetLamports: 1_000_000_000n,
    config: {
      enabled: true,
      bps: 100,
      treasury: "Treasury111111111111111111111111111111111"
    }
  });

  assert.equal(split.enabled, true);
  assert.equal(split.feeLamports, 10_000_000n);
  assert.equal(split.tradeLamports, 990_000_000n);
  assert.deepEqual(buildPlatformFeeTransferInstruction({ split, fromPubkey: "Wallet111" }), {
    kind: "system-transfer",
    fromPubkey: "Wallet111",
    toPubkey: "Treasury111111111111111111111111111111111",
    lamports: 10_000_000n
  });
});

test("platform fee supports custom bps and tiny amount floor rounding", () => {
  const split = calculatePlatformFeeSplit({
    action: "buy",
    budgetLamports: 99n,
    config: {
      enabled: true,
      bps: 50,
      treasury: "Treasury111111111111111111111111111111111"
    }
  });

  assert.equal(split.feeLamports, 0n);
  assert.equal(split.tradeLamports, 99n);
  assert.equal(formatPlatformFeeDisclosure(split), "platformFee=0 | tradeLamports=99 | budgetLamports=99");

  const larger = calculatePlatformFeeSplit({
    action: "buy",
    budgetLamports: 20_000n,
    config: {
      enabled: true,
      bps: 50,
      treasury: "Treasury111111111111111111111111111111111"
    }
  });
  assert.equal(larger.feeLamports, 100n);
  assert.equal(larger.tradeLamports, 19_900n);
});

test("platform fee uses validation hook to block invalid treasury strings", () => {
  const config = {
    enabled: true,
    bps: 100,
    treasury: "not-a-pubkey",
    validateTreasury: (treasury) => (treasury.startsWith("Treasury") ? null : "treasury is not a Solana pubkey")
  };

  assert.equal(platformFeeConfigBlockedReason(config), "treasury is not a Solana pubkey");

  const split = calculatePlatformFeeSplit({
    action: "buy",
    budgetLamports: 1000n,
    config
  });
  assert.equal(split.blockedReason, "treasury is not a Solana pubkey");
  assert.equal(split.feeLamports, 0n);
  assert.equal(split.tradeLamports, 1000n);
  assert.equal(buildPlatformFeeTransferInstruction({ split, fromPubkey: "Wallet" }), null);
});

test("platform fee splits sell proceeds without reducing the requested sell basis", () => {
  const split = calculatePlatformFeeSplit({
    action: "sell",
    budgetLamports: 1_000_000_000n,
    config: {
      enabled: true,
      bps: 100,
      treasury: "Treasury111111111111111111111111111111111"
    }
  });

  assert.equal(split.enabled, true);
  assert.equal(split.feeLamports, 10_000_000n);
  assert.equal(split.tradeLamports, 990_000_000n);
  assert.deepEqual(buildPlatformFeeTransferInstruction({ split, fromPubkey: "Wallet111" }), {
    kind: "system-transfer",
    fromPubkey: "Wallet111",
    toPubkey: "Treasury111111111111111111111111111111111",
    lamports: 10_000_000n
  });
});

test("platform fee validates bps and required treasury when enabled", () => {
  assert.match(platformFeeConfigBlockedReason({ enabled: true, bps: 10_001, treasury: "Treasury" }), /PLATFORM_FEE_BPS/);
  assert.equal(
    platformFeeConfigBlockedReason({ enabled: true, bps: 100, treasury: "" }),
    "PLATFORM_FEE_TREASURY is required when PLATFORM_FEE_ENABLED is true"
  );
});
