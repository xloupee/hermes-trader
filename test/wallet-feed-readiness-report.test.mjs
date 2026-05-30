import assert from "node:assert/strict";
import test from "node:test";
import {
  diagnosticWalletsFromEnv,
  mergeWallets,
  summarizeReadiness,
  walletsFromSubscribers
} from "../scripts/wallet-feed-readiness-report.mjs";

const copyWallet = "CopyWallet111111111111111111111111111111111";
const watchedWallet = "WatchedWallet11111111111111111111111111111";
const diagnosticWallet = "Diagnostic1111111111111111111111111111111";
const mint = "Mint111111111111111111111111111111111111111";
const observedAt = "2026-05-30T17:00:00.000Z";

function trade(overrides = {}) {
  return {
    observedAt,
    provider: "shredstream",
    targetWallet: copyWallet,
    action: "buy",
    mint,
    signature: "Signature111111111111111111111111111111111",
    input: { mint: "So11111111111111111111111111111111111111112", symbol: "SOL", amount: 0.1 },
    output: { mint, symbol: null, amount: 1000 },
    raw: {},
    ...overrides
  };
}

test("wallet feed readiness extracts and merges active subscriber wallets", () => {
  const wallets = walletsFromSubscribers([
    {
      watchedWallets: [{ address: watchedWallet, label: "watch" }],
      copyTradeWallets: [{ address: copyWallet, label: "copy" }]
    },
    {
      watchedWallets: [{ address: copyWallet, label: "also watched" }],
      copyTradeWallets: [{ address: copyWallet, label: "copy dupe" }]
    }
  ]);

  assert.equal(wallets.length, 2);
  const copy = wallets.find((wallet) => wallet.address === copyWallet);
  assert.deepEqual(copy.roles, ["copytrade", "watched"]);
  assert.equal(copy.chatCount, 3);

  assert.deepEqual(mergeWallets(wallets).find((wallet) => wallet.address === copyWallet).roles, ["copytrade", "watched"]);
});

test("wallet feed readiness parses diagnostic wallets from env", () => {
  assert.deepEqual(diagnosticWalletsFromEnv(`${diagnosticWallet}:diag`), [
    {
      address: diagnosticWallet,
      role: "diagnostic",
      label: "diag",
      chatCount: 1
    }
  ]);
});

test("wallet feed readiness summarizes copyable matched and isolated groups", () => {
  const activeWallets = mergeWallets([
    { address: copyWallet, role: "copytrade", label: "copy", chatCount: 1 },
    { address: watchedWallet, role: "watched", label: "watch", chatCount: 1 },
    { address: diagnosticWallet, role: "diagnostic", label: "diag", chatCount: 1 }
  ]);
  const summary = summarizeReadiness({
    activeWallets,
    rows: [
      trade(),
      trade({ provider: "geyser" }),
      trade({ signature: "RawOnly111111111111111111111111111111111" }),
      trade({ targetWallet: watchedWallet, signature: "Watched111111111111111111111111111111111" }),
      trade({ targetWallet: diagnosticWallet, raw: { diagnosticWallet: true } })
    ],
    sinceMs: Date.parse("2026-05-30T16:59:59.000Z"),
    role: "copytrade"
  });

  assert.equal(summary.selectedWallets.length, 1);
  assert.equal(summary.selectedRows.length, 3);
  assert.equal(summary.copyableRows.length, 3);
  assert.equal(summary.wallets[0].matchedCopyableGroups, 1);
  assert.equal(summary.wallets[0].isolatedCopyableGroups.get("shredstream"), 1);
});
