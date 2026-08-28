import assert from "node:assert/strict";
import { describe, test } from "node:test";
import {
  chainReportFromRustConfirmation,
  isSuccessfulMintBuy,
  sameMintBuysBetween
} from "../../../tools/jito-shredstream-rs/sync-local-copy-executions-to-supabase.mjs";

const MINT = "Mint111";

function blockTransaction(signature, {
  mint = MINT,
  owner = "Buyer111",
  pre = 0,
  post = 1,
  signer = true,
  err = null
} = {}) {
  return {
    transaction: {
      signatures: [signature],
      message: { accountKeys: [{ pubkey: owner, signer }] }
    },
    meta: {
      err,
      preTokenBalances: [{ accountIndex: 1, mint, owner, uiTokenAmount: { uiAmountString: String(pre) } }],
      postTokenBalances: [{ accountIndex: 1, mint, owner, uiTokenAmount: { uiAmountString: String(post) } }]
    }
  };
}

describe("same-mint buy distance", () => {
  test("classifies successful signer acquisitions only", () => {
    assert.equal(isSuccessfulMintBuy(blockTransaction("buy"), MINT), true);
    assert.equal(isSuccessfulMintBuy(blockTransaction("sell", { pre: 2, post: 1 }), MINT), false);
    assert.equal(isSuccessfulMintBuy(blockTransaction("failed", { err: { InstructionError: [0, "Custom"] } }), MINT), false);
    assert.equal(isSuccessfulMintBuy(blockTransaction("recipient", { signer: false }), MINT), false);
    assert.equal(isSuccessfulMintBuy(blockTransaction("other-mint", { mint: "Mint222" }), MINT), false);
  });

  test("counts buys strictly between target and copy in the same slot", async () => {
    const transactions = [
      blockTransaction("target"),
      blockTransaction("between-1"),
      blockTransaction("other-mint", { mint: "Mint222" }),
      blockTransaction("failed", { err: { InstructionError: [0, "Custom"] } }),
      blockTransaction("between-2"),
      blockTransaction("copy")
    ];
    const rpc = async (_method, [_slot, options]) => {
      assert.equal(options.transactionDetails, "full");
      return { transactions };
    };
    const result = await sameMintBuysBetween({
      targetSlot: 10,
      copySlot: 10,
      slotDelta: 0,
      targetTxIndex: 0,
      copyTxIndex: 5
    }, MINT, rpc);
    assert.deepEqual(result, { count: 2, unavailableReason: null });
  });

  test("counts the target tail, intermediate slots, and copy head", async () => {
    const blocks = new Map([
      [10, [blockTransaction("before-target"), blockTransaction("target"), blockTransaction("target-tail")]],
      [11, [blockTransaction("middle-1"), blockTransaction("middle-2")]],
      [12, [blockTransaction("copy-head"), blockTransaction("copy"), blockTransaction("after-copy")]]
    ]);
    const rpc = async (_method, [slot]) => ({ transactions: blocks.get(slot) });
    const result = await sameMintBuysBetween({
      targetSlot: 10,
      copySlot: 12,
      slotDelta: 2,
      targetTxIndex: 1,
      copyTxIndex: 1
    }, MINT, rpc);
    assert.deepEqual(result, { count: 4, unavailableReason: null });
  });

  test("enriches durable Rust position confirmations with the buy-only count", async () => {
    const transactions = [blockTransaction("target"), blockTransaction("between"), blockTransaction("copy")];
    const rpc = async (_method, [_slot, options]) => {
      assert.equal(options.transactionDetails, "full");
      return { transactions };
    };
    const report = await chainReportFromRustConfirmation({
      slot: 10,
      observedAction: "buy",
      observedSignature: "target",
      sendSignature: "copy",
      mint: MINT,
      sent: true,
      decision: "sent"
    }, {
      ok: true,
      status: "landed",
      confirmationSlot: 10,
      targetTxIndex: 0,
      copyTxIndex: 2,
      sameSlotTxDelta: 2
    }, rpc);
    assert.equal(report.blockPositionDiagnostics.sameMintBuysBetween, 1);
  });
});
