import assert from "node:assert/strict";
import test from "node:test";
import { providerSet, summarizeCoverage } from "../scripts/shredstream-wallet-coverage-report.mjs";

const wallet = "Trader111111111111111111111111111111111111";
const mint = "Mint111111111111111111111111111111111111111";
const signature = "ObservedSignature11111111111111111111111111";
const observedAt = "2026-05-30T16:00:00.000Z";
const receivedAtMs = Date.parse("2026-05-30T16:00:00.025Z");

function walletRow(overrides = {}) {
  return {
    observedAt,
    provider: "pumpportal",
    targetWallet: wallet,
    action: "buy",
    mint,
    input: { mint: "So11111111111111111111111111111111111111112", symbol: "SOL", amount: 0.1 },
    output: { mint, symbol: null, amount: 1000 },
    signature,
    timestamp: Date.parse(observedAt) / 1000,
    raw: {},
    ...overrides
  };
}

function shredRow(overrides = {}) {
  return {
    source: "shredstream",
    receivedAtMs,
    decodeStatus: "decoded",
    eventType: "buy",
    trader: wallet,
    mint,
    signature,
    ...overrides
  };
}

test("ShredStream wallet coverage matches wallet rows by signature wallet and mint", () => {
  const summary = summarizeCoverage({
    walletRows: [
      walletRow(),
      walletRow({ signature: "MissingSignature111111111111111111111111" }),
      walletRow({ provider: "helius", signature: "IgnoredProvider111111111111111111111111" })
    ],
    shredRows: [
      shredRow(),
      shredRow({ signature: "MissingSignature111111111111111111111111", trader: "OtherWallet1111111111111111111111111111111" }),
      shredRow({ signature: "BadStatus11111111111111111111111111111", decodeStatus: "unknown-discriminator" })
    ],
    providers: providerSet("pumpportal,geyser")
  });

  assert.equal(summary.walletRows.length, 2);
  assert.equal(summary.shredRows.length, 2);
  assert.equal(summary.matched.length, 1);
  assert.equal(summary.missing.length, 1);
  assert.equal(summary.missing[0].reason, "wallet_mismatch");
  assert.equal(summary.uncorroboratedShredRows.length, 1);
  assert.equal(summary.matched[0].deltaMs, 25);
  assert.equal(summary.providerCounts.get("pumpportal"), 2);
  assert.equal(summary.actionCounts.get("buy"), 2);
  assert.equal(summary.missingReasonCounts.get("wallet_mismatch"), 1);
});

test("ShredStream wallet coverage honors time windows", () => {
  const summary = summarizeCoverage({
    walletRows: [
      walletRow({ observedAt: "2026-05-30T15:59:59.999Z" }),
      walletRow({ signature: "InsideWindow111111111111111111111111111", observedAt })
    ],
    shredRows: [
      shredRow({ signature: "InsideWindow111111111111111111111111111" })
    ],
    sinceMs: Date.parse(observedAt),
    providers: providerSet("pumpportal")
  });

  assert.equal(summary.walletRows.length, 1);
  assert.equal(summary.matched.length, 1);
  assert.equal(summary.matched[0].wallet.signature, "InsideWindow111111111111111111111111111");
});

test("ShredStream wallet coverage classifies missing rows by signature evidence", () => {
  const rows = [
    walletRow({ signature: "AbsentSignature11111111111111111111111111" }),
    walletRow({ signature: "NoDecodedTrade1111111111111111111111111" }),
    walletRow({ signature: "MintMismatch1111111111111111111111111111" }),
    walletRow({ signature: "ActionMismatch11111111111111111111111111" })
  ];

  const summary = summarizeCoverage({
    walletRows: rows,
    shredRows: [
      shredRow({
        signature: "NoDecodedTrade1111111111111111111111111",
        decodeStatus: "unknown-discriminator",
        eventType: "unknown-pump",
        rawInstructionDiscriminator: "ffffffffffffffff"
      }),
      shredRow({
        signature: "MintMismatch1111111111111111111111111111",
        mint: "OtherMint111111111111111111111111111111111"
      }),
      shredRow({
        signature: "ActionMismatch11111111111111111111111111",
        eventType: "sell"
      })
    ],
    providers: providerSet("pumpportal")
  });

  assert.equal(summary.missing.length, 4);
  assert.equal(summary.missingReasonCounts.get("signature_absent"), 1);
  assert.equal(summary.missingReasonCounts.get("signature_present_no_decoded_trade"), 1);
  assert.equal(summary.missingReasonCounts.get("mint_mismatch"), 1);
  assert.equal(summary.missingReasonCounts.get("action_mismatch"), 1);
});

test("ShredStream wallet coverage can focus on copyable SOL-to-token buys", () => {
  const tokenToSolSignature = "TokenToSol1111111111111111111111111111111";
  const tokenToTokenSignature = "TokenToToken111111111111111111111111111";

  const summary = summarizeCoverage({
    walletRows: [
      walletRow(),
      walletRow({
        signature: tokenToSolSignature,
        action: "sell",
        input: { mint, symbol: null, amount: 1000 },
        output: { mint: "So11111111111111111111111111111111111111112", symbol: "SOL", amount: 0.1 }
      }),
      walletRow({
        signature: tokenToTokenSignature,
        input: { mint: "TokenA11111111111111111111111111111111111", symbol: null, amount: 1 },
        output: { mint: "TokenB11111111111111111111111111111111111", symbol: null, amount: 1 }
      })
    ],
    shredRows: [
      shredRow(),
      shredRow({ signature: tokenToSolSignature, eventType: "sell" }),
      shredRow({ signature: tokenToTokenSignature, quoteMint: "QuoteToken1111111111111111111111111111111" })
    ],
    providers: providerSet("pumpportal"),
    copyableOnly: true
  });

  assert.equal(summary.windowedWalletRows.length, 3);
  assert.equal(summary.copyableWalletRows.length, 1);
  assert.equal(summary.walletRows.length, 1);
  assert.equal(summary.windowedShredRows.length, 3);
  assert.equal(summary.copyableShredRows.length, 1);
  assert.equal(summary.shredRows.length, 1);
  assert.equal(summary.matched.length, 1);
  assert.equal(summary.missing.length, 0);
  assert.equal(summary.uncorroboratedShredRows.length, 0);
});

test("ShredStream wallet coverage reports raw-only copyable buys", () => {
  const rawOnlySignature = "RawOnly11111111111111111111111111111111111";

  const summary = summarizeCoverage({
    walletRows: [walletRow()],
    shredRows: [
      shredRow(),
      shredRow({ signature: rawOnlySignature })
    ],
    providers: providerSet("pumpportal"),
    copyableOnly: true
  });

  assert.equal(summary.walletRows.length, 1);
  assert.equal(summary.shredRows.length, 2);
  assert.equal(summary.matched.length, 1);
  assert.equal(summary.uncorroboratedShredRows.length, 1);
  assert.equal(summary.uncorroboratedShredRows[0].signature, rawOnlySignature);
});
