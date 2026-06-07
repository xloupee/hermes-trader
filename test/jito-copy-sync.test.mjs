import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { readJsonl } from "../tools/jito-shredstream-rs/sync-local-copy-executions-to-supabase.mjs";

test("sync recent limit counts local execution rows after filtering auxiliary rows", async () => {
  const dir = await mkdtemp(join(tmpdir(), "jito-copy-sync-"));
  const path = join(dir, "executions.jsonl");
  try {
    const rows = [
      {
        schema: "copytrade.localExecution.v1",
        observedSignature: "target-buy-a",
        observedAction: "buy",
        mint: "mint-a"
      },
      {
        schema: "copytrade.transactionConfirmation.v1",
        observedSignature: "target-buy-a",
        transactionRole: "copy_buy",
        signature: "copy-buy-a"
      },
      {
        schema: "copytrade.rustTrailingSell.v1",
        observedSignature: "target-buy-a",
        stepIndex: 0,
        sendSignature: "sell-a"
      },
      {
        schema: "copytrade.transactionConfirmation.v1",
        observedSignature: "target-buy-a",
        transactionRole: "rust_trailing_sell",
        stepIndex: 0,
        signature: "sell-a"
      },
      {
        schema: "copytrade.localExecution.v1",
        observedSignature: "target-buy-b",
        observedAction: "buy",
        mint: "mint-b"
      },
      {
        schema: "copytrade.transactionConfirmation.v1",
        observedSignature: "target-buy-b",
        transactionRole: "copy_buy",
        signature: "copy-buy-b"
      }
    ];
    await writeFile(path, `${rows.map((row) => JSON.stringify(row)).join("\n")}\n`);

    const recent = readJsonl(path, { recentLimit: 1 });

    assert.equal(recent.length, 1);
    assert.equal(recent[0].observedSignature, "target-buy-b");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
