import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  buildSql,
  readJsonl
} from "../tools/jito-shredstream-rs/sync-local-copy-executions-to-supabase.mjs";

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
