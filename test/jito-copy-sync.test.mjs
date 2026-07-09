import assert from "node:assert/strict";
import { appendFile, mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  buildRestRows,
  buildSql,
  DurableJsonlTail,
  readJsonl,
  syncRows,
  syncViaSupabaseRest
} from "../tools/jito-shredstream-rs/sync-local-copy-executions-to-supabase.mjs";

function executionRow(observedSignature) {
  return {
    schema: "copytrade.localExecution.v1",
    provider: "shredstream",
    observedSignature,
    observedWallet: "wallet",
    copyWallet: "copy-wallet",
    observedAction: "buy",
    mint: `mint-${observedSignature}`
  };
}

test("durable JSONL tail resumes from committed byte offset and waits for partial lines", async () => {
  const dir = await mkdtemp(join(tmpdir(), "jito-copy-tail-"));
  const path = join(dir, "executions.jsonl");
  const cursorPath = join(dir, "cursor.json");
  try {
    const first = JSON.stringify(executionRow("first"));
    const second = JSON.stringify(executionRow("second"));
    await writeFile(path, `${first}\n${second.slice(0, 20)}`);

    const tail = new DurableJsonlTail(path, { cursorPath, initialRecentLines: 0 });
    const firstBatch = await tail.readBatch();
    assert.deepEqual(firstBatch.rows.map((row) => row.observedSignature), ["first"]);
    assert.equal(firstBatch.partial, true);
    await tail.commit(firstBatch);

    const restarted = new DurableJsonlTail(path, { cursorPath, initialRecentLines: 0 });
    const partialBatch = await restarted.readBatch();
    assert.equal(partialBatch.rows.length, 0);
    assert.equal(partialBatch.partial, true);

    await appendFile(path, `${second.slice(20)}\n`);
    const completedBatch = await restarted.readBatch();
    assert.deepEqual(completedBatch.rows.map((row) => row.observedSignature), ["second"]);
    await restarted.commit(completedBatch);

    const cursor = JSON.parse(await readFile(cursorPath, "utf8"));
    assert.equal(cursor.offset, Buffer.byteLength(`${first}\n${second}\n`));
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("durable JSONL tail bootstrap limit counts local executions rather than sidecars", async () => {
  const dir = await mkdtemp(join(tmpdir(), "jito-copy-tail-"));
  const path = join(dir, "executions.jsonl");
  try {
    const rows = [
      executionRow("old"),
      { schema: "copytrade.transactionConfirmation.v1", observedSignature: "old", transactionRole: "copy_buy" },
      executionRow("new"),
      { schema: "copytrade.transactionConfirmation.v1", observedSignature: "new", transactionRole: "copy_buy" }
    ];
    await writeFile(path, `${rows.map((row) => JSON.stringify(row)).join("\n")}\n`);
    const tail = new DurableJsonlTail(path, { initialRecentLines: 1 });
    const batch = await tail.readBatch();
    assert.deepEqual(batch.rows.map((row) => row.observedSignature), ["new", "new"]);
  } finally {
    await rm(dir, { recursive: true, force: true });
    await rm(`${path}.sync-cursor.json`, { force: true });
  }
});

test("durable JSONL tail bounds batches and skips malformed complete records", async () => {
  const dir = await mkdtemp(join(tmpdir(), "jito-copy-tail-"));
  const path = join(dir, "executions.jsonl");
  const cursorPath = join(dir, "cursor.json");
  const deadLetterPath = join(dir, "dead-letter.jsonl");
  try {
    const rows = [executionRow("one"), executionRow("two"), executionRow("three")];
    await writeFile(path, `${JSON.stringify(rows[0])}\nnot-json\n${JSON.stringify(rows[1])}\n${JSON.stringify(rows[2])}\n`);
    const tail = new DurableJsonlTail(path, {
      cursorPath,
      deadLetterPath,
      initialRecentLines: 0,
      maxBatchRows: 2
    });

    const firstBatch = await tail.readBatch();
    assert.deepEqual(firstBatch.rows.map((row) => row.observedSignature), ["one"]);
    assert.equal(firstBatch.malformed.length, 1);
    assert.equal(firstBatch.hasMore, true);
    await tail.persistMalformed(firstBatch);
    await tail.commit(firstBatch);
    const [deadLetter] = (await readFile(deadLetterPath, "utf8")).trim().split("\n").map(JSON.parse);
    assert.equal(deadLetter.schema, "copytrade.syncDeadLetter.v1");
    assert.equal(Buffer.from(deadLetter.rawBase64, "base64").toString("utf8"), "not-json");

    const secondBatch = await tail.readBatch();
    assert.deepEqual(secondBatch.rows.map((row) => row.observedSignature), ["two", "three"]);
    await tail.commit(secondBatch);
    assert.equal((await tail.readBatch()).rows.length, 0);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("durable JSONL tail advances past an oversized malformed record", async () => {
  const dir = await mkdtemp(join(tmpdir(), "jito-copy-tail-"));
  const path = join(dir, "executions.jsonl");
  const cursorPath = join(dir, "cursor.json");
  try {
    await writeFile(path, `${"x".repeat(512)}\n${JSON.stringify(executionRow("after-oversized"))}\n`);
    const tail = new DurableJsonlTail(path, {
      cursorPath,
      initialRecentLines: 0,
      maxBatchBytes: 256
    });
    const seen = [];
    let malformed = 0;
    for (let attempt = 0; attempt < 10 && seen.length === 0; attempt += 1) {
      const batch = await tail.readBatch();
      malformed += batch.malformed.length;
      seen.push(...batch.rows.map((row) => row.observedSignature));
      await tail.commit(batch);
    }
    assert.equal(malformed, 1);
    assert.deepEqual(seen, ["after-oversized"]);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("durable JSONL tail recovers enrichment work committed after the raw upsert", async () => {
  const dir = await mkdtemp(join(tmpdir(), "jito-copy-tail-"));
  const path = join(dir, "executions.jsonl");
  const cursorPath = join(dir, "cursor.json");
  try {
    const row = executionRow("pending");
    await writeFile(path, `${JSON.stringify(row)}\n`);
    const tail = new DurableJsonlTail(path, { cursorPath, initialRecentLines: 0 });
    const batch = await tail.readBatch();
    await tail.commit(batch, { pendingEnrichmentRows: batch.rows });

    const restarted = new DurableJsonlTail(path, { cursorPath, initialRecentLines: 0 });
    await restarted.initialize();
    assert.deepEqual(
      restarted.pendingEnrichmentRows().map((pending) => pending.observedSignature),
      ["pending"]
    );
    await restarted.acknowledgeEnrichment(batch.rows);
    assert.equal(restarted.pendingEnrichmentRows().length, 0);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("durable JSONL tail resets safely on truncation and rotation", async () => {
  const dir = await mkdtemp(join(tmpdir(), "jito-copy-tail-"));
  const path = join(dir, "executions.jsonl");
  const cursorPath = join(dir, "cursor.json");
  try {
    await writeFile(path, `${JSON.stringify(executionRow("before"))}\n`);
    const tail = new DurableJsonlTail(path, { cursorPath, initialRecentLines: 0 });
    const initial = await tail.readBatch();
    await tail.commit(initial);

    await writeFile(path, `${JSON.stringify(executionRow("truncated"))}\n`);
    const truncated = await tail.readBatch();
    assert.equal(truncated.reset, true);
    assert.deepEqual(truncated.rows.map((row) => row.observedSignature), ["truncated"]);
    await tail.commit(truncated);

    await rename(path, `${path}.1`);
    await writeFile(path, `${JSON.stringify(executionRow("rotated"))}\n`);
    const rotated = await tail.readBatch();
    assert.equal(rotated.reset, true);
    assert.deepEqual(rotated.rows.map((row) => row.observedSignature), ["rotated"]);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("raw REST rows preserve base execution fields without overwriting enrichment columns", async () => {
  const [record] = await buildRestRows([executionRow("raw")], { enrich: false });
  assert.equal(record.observed_signature, "raw");
  assert.equal(record.raw_execution.observedSignature, "raw");
  for (const column of ["copy_slot", "tx_delta", "chain_report", "gross_copy_spend_sol"]) {
    assert.equal(Object.hasOwn(record, column), false);
  }
});

test("raw REST writes ignore duplicates while enriched writes merge duplicates", async () => {
  const originalFetch = globalThis.fetch;
  const originalUrl = process.env.SUPABASE_URL;
  const originalKey = process.env.SUPABASE_SERVICE_ROLE_KEY;
  const preferences = [];
  process.env.SUPABASE_URL = "https://example.supabase.co";
  process.env.SUPABASE_SERVICE_ROLE_KEY = "test-service-role";
  globalThis.fetch = async (_url, init) => {
    preferences.push(init.headers.prefer);
    return new Response(null, { status: 204 });
  };
  try {
    await syncRows([executionRow("raw")], { enrich: false });
    await syncViaSupabaseRest([{ observed_signature: "enriched" }], { mergeDuplicates: true });
    assert.deepEqual(preferences, [
      "resolution=ignore-duplicates,return=minimal",
      "resolution=merge-duplicates,return=minimal"
    ]);
  } finally {
    globalThis.fetch = originalFetch;
    if (originalUrl === undefined) delete process.env.SUPABASE_URL;
    else process.env.SUPABASE_URL = originalUrl;
    if (originalKey === undefined) delete process.env.SUPABASE_SERVICE_ROLE_KEY;
    else process.env.SUPABASE_SERVICE_ROLE_KEY = originalKey;
  }
});

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

test("sync can combine recent rows with older pending position refresh rows", async () => {
  const dir = await mkdtemp(join(tmpdir(), "jito-copy-sync-"));
  const path = join(dir, "executions.jsonl");
  try {
    const rows = [
      {
        schema: "copytrade.localExecution.v1",
        provider: "shredstream",
        observedSignature: "target-buy-pending",
        observedWallet: "wallet",
        copyWallet: "copy-wallet",
        observedAction: "buy",
        mint: "mint-pending",
        sendSignature: "copy-pending",
        sent: true,
        decision: "sent"
      },
      {
        schema: "copytrade.localExecution.v1",
        provider: "shredstream",
        observedSignature: "target-buy-complete",
        observedWallet: "wallet",
        copyWallet: "copy-wallet",
        observedAction: "buy",
        mint: "mint-complete",
        sendSignature: "copy-complete",
        sent: true,
        decision: "sent"
      },
      {
        schema: "copytrade.transactionConfirmation.v1",
        provider: "shredstream",
        observedSignature: "target-buy-complete",
        copyWallet: "copy-wallet",
        mint: "mint-complete",
        transactionRole: "copy_buy",
        signature: "copy-complete",
        targetTxIndex: 1,
        copyTxIndex: 2,
        txDelta: 1
      },
      {
        schema: "copytrade.localExecution.v1",
        provider: "shredstream",
        observedSignature: "target-buy-new",
        observedWallet: "wallet",
        copyWallet: "copy-wallet",
        observedAction: "buy",
        mint: "mint-new",
        sendSignature: "copy-new",
        sent: true,
        decision: "sent"
      }
    ];
    await writeFile(path, `${rows.map((row) => JSON.stringify(row)).join("\n")}\n`);

    const selected = readJsonl(path, { recentLimit: 1, pendingPositionLimit: 10 });

    assert.deepEqual(
      selected.map((row) => row.observedSignature).sort(),
      ["target-buy-new", "target-buy-pending"]
    );
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("sync SQL includes landing telemetry and account priority cache fields", async () => {
  const sql = await buildSql([
    {
      schema: "copytrade.localExecution.v1",
      observedAtMs: 1_788_000_000_000,
      executionAtMs: 1_788_000_000_010,
      provider: "shredstream",
      source: "jito-proxy",
      endpoint: "local",
      observedWallet: "observed-wallet",
      copyWallet: "copy-wallet",
      observedSignature: "observed-signature",
      sendSignature: "copy-signature",
      slot: 123,
      selectedRoute: "flashx_pump",
      routeLayout: "flashx_pump_buy_v1",
      mint: "mint",
      observedAction: "buy",
      decision: "sent",
      signed: true,
      sent: true,
      sendEnabled: true,
      simulationRequested: false,
      instructionCount: 6,
      signedTxBytes: 812,
      writableAccountCount: 17,
      computeUnitLimit: 400_000,
      selectedTipAccount: "tip-account",
      sourceComputeUnitLimit: 300_000,
      sourceComputeUnitPriceMicroLamports: 2_500_000,
      computeUnitsConsumed: 188_000,
      costUnits: 205_000,
      transactionMetaError: null,
      blockhash: "blockhash",
      blockhashSourceRpc: "state-rpc",
      blockhashCommitment: "processed",
      blockhashContextSlot: 122,
      blockhashAgeMs: 25,
      blockhashSelectionStrategy: "highest_context_slot",
      accountPriorityFeeEnabled: true,
      accountPriorityFeeMicroLamports: 3_000_000,
      accountPriorityFeeAgeMs: 750,
      accountPriorityFeeSampleCount: 150,
      accountPriorityFeeAccountCount: 17,
      accountPriorityFeeApplied: true,
      accountPriorityFeeReason: "applied"
    }
  ]);

  for (const column of [
    "signed_tx_bytes",
    "writable_account_count",
    "selected_tip_account",
    "source_compute_unit_price_micro_lamports",
    "compute_units_consumed",
    "cost_units",
    "blockhash_source_rpc",
    "blockhash_commitment",
    "blockhash_context_slot",
    "blockhash_age_ms",
    "account_priority_fee_enabled",
    "account_priority_fee_micro_lamports",
    "account_priority_fee_applied",
    "account_priority_fee_reason"
  ]) {
    assert.match(sql, new RegExp(`\\b${column}\\b`));
  }
  assert.match(sql, /'tip-account'/);
  assert.match(sql, /\b3000000\b/);
  assert.match(sql, /'highest_context_slot'/);
  assert.match(sql, /'applied'/);
});
