import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { evaluatePromotionGate } from "../scripts/shredstream-promotion-gate.mjs";

function map(entries) {
  return new Map(entries);
}

test("ShredStream promotion gate fails closed without real samples", () => {
  const result = evaluatePromotionGate({
    selectedWallets: [{ address: "Wallet111111111111111111111111111111111" }],
    copyableRows: [],
    wallets: [
      {
        matchedCopyableGroups: 0,
        copyableProviderCounts: map([])
      }
    ]
  });

  assert.equal(result.ok, false);
  assert.match(result.failures.join("; "), /copyable buys 0 < 1/);
  assert.match(result.failures.join("; "), /ShredStream copyable buys 0 < 1/);
  assert.match(result.failures.join("; "), /matched copyable groups 0 < 1/);
});

test("ShredStream promotion gate passes when real matched ShredStream samples exist", () => {
  const result = evaluatePromotionGate({
    selectedWallets: [{ address: "Wallet111111111111111111111111111111111" }],
    copyableRows: [{ provider: "shredstream" }],
    wallets: [
      {
        matchedCopyableGroups: 1,
        copyableProviderCounts: map([["shredstream", 1], ["geyser", 1]])
      }
    ]
  });

  assert.equal(result.ok, true);
  assert.deepEqual(result.failures, []);
  assert.equal(result.metrics.shredstreamCopyableBuys, 1);
});

test("ShredStream promotion gate ignores ambient threshold env defaults", () => {
  const result = spawnSync(process.execPath, ["scripts/shredstream-promotion-gate.mjs", "--path=/tmp/does-not-matter.jsonl"], {
    cwd: new URL("..", import.meta.url),
    env: {
      ...process.env,
      GATE_MIN_ACTIVE_WALLETS: "0",
      GATE_MIN_COPYABLE_BUYS: "0",
      GATE_MIN_SHREDSTREAM_COPYABLE_BUYS: "0",
      GATE_MIN_MATCHED_COPYABLE_GROUPS: "0"
    },
    encoding: "utf8"
  });

  assert.notEqual(result.status, 0);
  assert.doesNotMatch(result.stderr + result.stdout, /Result=PASS/);
});
