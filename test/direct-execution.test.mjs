import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import BN from "bn.js";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import { getAssociatedTokenAddressSync, NATIVE_MINT, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { buildDirectAutoTransactionPayload } from "../dist/direct-auto.js";
import { buildDirectPumpTransaction } from "../dist/direct-pump.js";
import { buildDirectPumpSwapTransaction } from "../dist/direct-pumpswap.js";
import { sendDirectTransaction } from "../dist/direct-sender.js";
import {
  buildDirectPumpSwapLocalBuyInstructions,
  buildDirectPumpLocalBuyInstructions,
  buildDirectSolanaSendConnections,
  directAutoProviderOrderForRequest,
  extractPumpBuyV2CreatorVaultFromTransaction,
  fetchDirectPumpBuyState,
  primeDirectPumpFastBuyState,
  refreshDirectPumpFastBuyStateReserves,
  resolveMintTokenProgram,
  sendSolanaDirectTransaction,
  warmDirectSolanaBlockhash
} from "../dist/direct-solana.js";
import { maxQuoteLamportsForSlippageCap } from "../dist/direct-budget.js";
import { tradeExecutionSkippedResult } from "../dist/trade-execution.js";

const require = createRequire(import.meta.url);
const { PUMP_SDK } = require("@pump-fun/pump-sdk");
const {
  canonicalPumpPoolPda,
  PUMP_AMM_PROGRAM_ID,
  PUMP_AMM_SDK,
  buyQuoteInput: pumpSwapBuyQuoteInput
} = require("@pump-fun/pump-swap-sdk");

const baseRequest = {
  action: "buy",
  mint: "Mint111111111111111111111111111111111111111",
  amount: 990_000_000n,
  amountBasis: "sol",
  slippagePercent: 10,
  priorityFeeSol: 0.00005,
  walletPublicKey: "Wallet111111111111111111111111111111111111"
};

function instructionShape(instruction) {
  return {
    programId: instruction.programId.toBase58(),
    data: instruction.data.toString("hex"),
    keys: instruction.keys.map((key) => ({
      pubkey: key.pubkey.toBase58(),
      isWritable: key.isWritable,
      isSigner: key.isSigner
    }))
  };
}

const liveGate = {
  provider: "direct-pump",
  copyTradeEnabled: true,
  copyTradeDryRun: false,
  directExecutionEnabled: true,
  directExecutionLiveEnabled: true
};

test("direct Pump builder wraps injected SDK buy instructions and route metadata", async () => {
  const calls = [];
  const result = await buildDirectPumpTransaction({
    sdk: {
      buyInstructions: (input) => {
        calls.push(input);
        return [{ kind: "pump-buy", mint: input.mint, amount: input.amount }];
      }
    },
    request: {
      ...baseRequest,
      platformFeeInstruction: { kind: "system-transfer", lamports: 10_000_000n }
    },
    loadState: () => ({ ok: true, state: { bondingCurve: "curve-1" } })
  });

  assert.equal(result.provider, "direct-pump");
  assert.equal(result.route.route, "pump-bonding-curve");
  assert.equal(result.instructions.length, 2);
  assert.equal(result.instructions[0].kind, "system-transfer");
  assert.equal(result.instructions[1].kind, "pump-buy");
  assert.equal(calls[0].slippagePercent, 10);
  assert.equal(calls[0].state.bondingCurve, "curve-1");
});

test("direct Pump builder safely skips when bonding-curve state is missing", async () => {
  const result = await buildDirectPumpTransaction({
    sdk: {
      buyInstructions: () => [{ kind: "pump-buy" }]
    },
    request: baseRequest,
    loadState: () => ({ ok: false, reason: "bonding curve account not found" })
  });

  assert.equal(result.status, "skipped");
  assert.match(result.errorText, /bonding curve account not found/);
});

test("direct Pump builder maps injected SDK errors to failed provider-neutral results", async () => {
  const result = await buildDirectPumpTransaction({
    sdk: {
      buyInstructions: () => {
        throw new Error("invalid mint");
      }
    },
    request: baseRequest
  });

  assert.equal(result.status, "failed");
  assert.match(result.errorText, /invalid mint/);
});

test("local Solana Pump buy builder matches SDK legacy and Token-2022 instructions", async () => {
  const originalRandom = Math.random;
  Math.random = () => 0;

  try {
    const mint = Keypair.generate().publicKey;
    const user = Keypair.generate().publicKey;
    const creator = Keypair.generate().publicKey;
    const global = {
      feeRecipient: new PublicKey("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV"),
      feeRecipients: [new PublicKey("7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ")],
      reservedFeeRecipient: new PublicKey("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV"),
      reservedFeeRecipients: [],
      buybackFeeRecipients: [],
      buybackBasisPoints: new BN(0)
    };
    const bondingCurve = {
      creator,
      isMayhemMode: false,
      quoteMint: NATIVE_MINT
    };
    const baseInput = {
      global,
      bondingCurveAccountInfo: { data: Buffer.alloc(0) },
      bondingCurve,
      associatedUserAccountInfo: { data: Buffer.alloc(0) },
      mint,
      user,
      amount: new BN(123),
      slippage: 1
    };

    const legacySdk = await PUMP_SDK.buyInstructions({
      ...baseInput,
      solAmount: new BN(456),
      tokenProgram: TOKEN_PROGRAM_ID
    });
    const legacyLocal = buildDirectPumpLocalBuyInstructions({
      global,
      bondingCurve,
      associatedUserAccountInfo: baseInput.associatedUserAccountInfo,
      mint,
      user,
      amount: baseInput.amount,
      quoteAmount: new BN(456),
      slippage: baseInput.slippage,
      tokenProgram: TOKEN_PROGRAM_ID
    });

    assert.deepEqual(
      legacyLocal.map(instructionShape),
      legacySdk.map(instructionShape)
    );

    const token2022Sdk = await PUMP_SDK.buyV2Instructions({
      ...baseInput,
      quoteAmount: new BN(789),
      tokenProgram: TOKEN_2022_PROGRAM_ID,
      quoteTokenProgram: TOKEN_PROGRAM_ID
    });
    const token2022Local = buildDirectPumpLocalBuyInstructions({
      global,
      bondingCurve,
      associatedUserAccountInfo: baseInput.associatedUserAccountInfo,
      mint,
      user,
      amount: baseInput.amount,
      quoteAmount: new BN(789),
      slippage: baseInput.slippage,
      tokenProgram: TOKEN_2022_PROGRAM_ID,
      quoteTokenProgram: TOKEN_PROGRAM_ID
    });

    assert.deepEqual(
      token2022Local.map(instructionShape),
      token2022Sdk.map(instructionShape)
    );
  } finally {
    Math.random = originalRandom;
  }
});

test("local PumpSwap buy builder matches SDK quote-input instructions", async () => {
  const originalRandom = Math.random;
  Math.random = () => 0;

  try {
    const user = Keypair.generate().publicKey;
    const baseMint = Keypair.generate().publicKey;
    const creator = Keypair.generate().publicKey;
    const coinCreator = Keypair.generate().publicKey;
    const poolKey = canonicalPumpPoolPda(baseMint, NATIVE_MINT);
    const quoteMint = NATIVE_MINT;
    const pool = {
      poolBump: 0,
      index: 0,
      creator,
      baseMint,
      quoteMint,
      lpMint: Keypair.generate().publicKey,
      poolBaseTokenAccount: getAssociatedTokenAddressSync(baseMint, poolKey, true, TOKEN_PROGRAM_ID),
      poolQuoteTokenAccount: getAssociatedTokenAddressSync(quoteMint, poolKey, true, TOKEN_PROGRAM_ID),
      lpSupply: new BN(0),
      coinCreator,
      isMayhemMode: false,
      isCashbackCoin: false
    };
    const state = {
      globalConfig: {
        admin: Keypair.generate().publicKey,
        lpFeeBasisPoints: new BN(20),
        protocolFeeBasisPoints: new BN(5),
        disableFlags: 0,
        protocolFeeRecipients: [Keypair.generate().publicKey],
        coinCreatorFeeBasisPoints: new BN(5),
        adminSetCoinCreatorAuthority: Keypair.generate().publicKey,
        whitelistPda: Keypair.generate().publicKey,
        reservedFeeRecipient: Keypair.generate().publicKey,
        mayhemModeEnabled: false,
        reservedFeeRecipients: [Keypair.generate().publicKey],
        buybackFeeRecipients: [Keypair.generate().publicKey],
        buybackBasisPoints: new BN(0)
      },
      feeConfig: null,
      poolKey,
      poolAccountInfo: {
        data: Buffer.alloc(300),
        executable: false,
        lamports: 1,
        owner: PUMP_AMM_PROGRAM_ID
      },
      pool,
      poolBaseAmount: new BN(1_000_000_000),
      poolQuoteAmount: new BN(1_000_000_000),
      baseTokenProgram: TOKEN_PROGRAM_ID,
      quoteTokenProgram: TOKEN_PROGRAM_ID,
      baseMint,
      baseMintAccount: { supply: 1_000_000_000n },
      user,
      userBaseTokenAccount: getAssociatedTokenAddressSync(baseMint, user, true, TOKEN_PROGRAM_ID),
      userQuoteTokenAccount: getAssociatedTokenAddressSync(quoteMint, user, true, TOKEN_PROGRAM_ID),
      userBaseAccountInfo: null,
      userQuoteAccountInfo: null
    };
    const quoteAmount = new BN(1_000_000);
    const slippage = 1;
    const quote = pumpSwapBuyQuoteInput({
      quote: quoteAmount,
      slippage,
      baseReserve: state.poolBaseAmount,
      quoteReserve: state.poolQuoteAmount,
      baseMintAccount: state.baseMintAccount,
      baseMint: state.baseMint,
      coinCreator: state.pool.coinCreator,
      creator: state.pool.creator,
      feeConfig: state.feeConfig,
      globalConfig: state.globalConfig
    });

    const sdkInstructions = await PUMP_AMM_SDK.buyQuoteInput(state, quoteAmount, slippage);
    const localInstructions = buildDirectPumpSwapLocalBuyInstructions({
      state,
      baseOut: quote.base,
      maxQuoteIn: quote.maxQuote
    });

    assert.deepEqual(
      localInstructions.map(instructionShape),
      sdkInstructions.map(instructionShape)
    );
  } finally {
    Math.random = originalRandom;
  }
});

test("direct PumpSwap builder safely skips missing canonical pool", async () => {
  const result = await buildDirectPumpSwapTransaction({
    sdk: {
      buyQuoteInput: () => [{ kind: "pumpswap-buy" }]
    },
    request: baseRequest,
    loadPool: () => ({ ok: false, reason: "pool not found" })
  });

  assert.equal(result.status, "skipped");
  assert.match(result.errorText, /pool not found/);
});

test("direct PumpSwap builder records pool metadata and WSOL handling instructions", async () => {
  const calls = [];
  const result = await buildDirectPumpSwapTransaction({
    sdk: {
      sellBaseInput: (input) => {
        calls.push(input);
        return [{ kind: "pumpswap-sell", poolAddress: input.poolAddress }];
      }
    },
    request: {
      ...baseRequest,
      action: "sell",
      amount: "25%",
      amountBasis: "percent"
    },
    loadPool: () => ({
      ok: true,
      pool: {
        poolAddress: "Pool1111111111111111111111111111111111111",
        state: { feeBps: 30 },
        needsWrappedSolAccount: true
      }
    })
  });

  assert.equal(result.provider, "direct-pumpswap");
  assert.equal(result.route.route, "pumpswap-amm");
  assert.equal(result.route.poolAddress, "Pool1111111111111111111111111111111111111");
  assert.deepEqual(result.instructions.map((instruction) => instruction.kind), [
    "create-wsol-account",
    "sync-wsol-account",
    "pumpswap-sell",
    "close-wsol-account"
  ]);
  assert.equal(calls[0].state.feeBps, 30);
  assert.equal(calls[0].amountBasis, "percent");
});

test("direct PumpSwap builder appends sell platform fee after sell handling", async () => {
  const result = await buildDirectPumpSwapTransaction({
    sdk: {
      sellBaseInput: () => [{ kind: "pumpswap-sell" }]
    },
    request: {
      ...baseRequest,
      action: "sell",
      amount: "100%",
      amountBasis: "percent",
      platformFeeInstruction: { kind: "system-transfer", lamports: 10_000_000n }
    },
    loadPool: () => ({
      ok: true,
      pool: {
        poolAddress: "Pool1111111111111111111111111111111111111",
        needsWrappedSolAccount: true
      }
    })
  });

  assert.deepEqual(result.instructions.map((instruction) => instruction.kind), [
    "create-wsol-account",
    "sync-wsol-account",
    "pumpswap-sell",
    "close-wsol-account",
    "system-transfer"
  ]);
});

function payload(overrides = {}) {
  return {
    provider: "direct-pump",
    route: {
      provider: "direct-pump",
      route: "pump-bonding-curve",
      mint: baseRequest.mint,
      walletPublicKey: baseRequest.walletPublicKey,
      poolAddress: null,
      priorityFeeSol: 0.00005,
      slippagePercent: 10,
      amount: "990000000",
      amountBasis: "sol"
    },
    instructions: [{ kind: "pump-buy" }],
    signers: [],
    metadata: { idempotencyKey: "chat:wallet:mint:buy" },
    ...overrides
  };
}

function signer(overrides = {}) {
  return {
    publicKey: baseRequest.walletPublicKey,
    signTransaction: (transaction) => ({
      ...transaction,
      serialize: () => new Uint8Array([1, 2, 3])
    }),
    ...overrides
  };
}

test("direct-auto probes PumpSwap before falling back to Pump", async () => {
  const attempts = [];
  const result = await buildDirectAutoTransactionPayload({
    attempts: [
      {
        provider: "direct-pumpswap",
        build: async () => {
          attempts.push("direct-pumpswap");
          return tradeExecutionSkippedResult({
            provider: "direct-pumpswap",
            route: "pumpswap-amm",
            reason: "pool not found"
          });
        }
      },
      {
        provider: "direct-pump",
        build: async () => {
          attempts.push("direct-pump");
          return payload();
        }
      }
    ]
  });

  assert.deepEqual(attempts, ["direct-pumpswap", "direct-pump"]);
  assert.equal(result.provider, "direct-pump");
  assert.equal(result.route.route, "pump-bonding-curve");
});

test("direct-auto records thrown route-builder failures and continues to the next route", async () => {
  const attempts = [];
  const result = await buildDirectAutoTransactionPayload({
    attempts: [
      {
        provider: "direct-pumpswap",
        build: async () => {
          attempts.push("direct-pumpswap");
          throw new Error("invalid mint");
        }
      },
      {
        provider: "direct-pump",
        build: async () => {
          attempts.push("direct-pump");
          return payload();
        }
      }
    ]
  });

  assert.deepEqual(attempts, ["direct-pumpswap", "direct-pump"]);
  assert.equal(result.provider, "direct-pump");
});

test("direct-auto prefers Pump first when observed route hints point at bonding curve", () => {
  assert.deepEqual(
    directAutoProviderOrderForRequest({ metadata: { observedPool: "pump" } }),
    ["direct-pump", "direct-pumpswap"]
  );
  assert.deepEqual(
    directAutoProviderOrderForRequest({ metadata: { observedSource: "pump_fun" } }),
    ["direct-pump", "direct-pumpswap"]
  );
});

test("direct-auto preserves PumpSwap first for AMM hints and unknown routes", () => {
  assert.deepEqual(
    directAutoProviderOrderForRequest({ metadata: { observedPool: "pump-amm" } }),
    ["direct-pumpswap", "direct-pump"]
  );
  assert.deepEqual(
    directAutoProviderOrderForRequest({ metadata: { observedPool: "pumpswap" } }),
    ["direct-pumpswap", "direct-pump"]
  );
  assert.deepEqual(
    directAutoProviderOrderForRequest({ metadata: { observedPool: "unknown" } }),
    ["direct-pumpswap", "direct-pump"]
  );
});

test("direct sender fails closed when direct live gates are missing", async () => {
  const result = await sendDirectTransaction({
    connection: {},
    signer: signer(),
    payload: payload(),
    config: {
      gate: {
        provider: "direct-pump",
        copyTradeEnabled: false,
        copyTradeDryRun: true
      }
    }
  });

  assert.equal(result.status, "skipped");
  assert.match(result.errorText, /COPY_TRADE_ENABLED is not true/);
});

test("direct sender blocks simulation failures before signing or sending", async () => {
  let signed = false;
  let sent = false;
  const result = await sendDirectTransaction({
    connection: {
      getLatestBlockhash: () => ({ blockhash: "blockhash-1", lastValidBlockHeight: 123 }),
      simulateTransaction: () => ({ err: { InstructionError: [0, "Custom"] }, logs: ["failed"] }),
      sendRawTransaction: () => {
        sent = true;
        return "signature-1";
      }
    },
    signer: signer({
      signTransaction: () => {
        signed = true;
        throw new Error("should not sign");
      }
    }),
    payload: payload(),
    config: { gate: liveGate }
  });

  assert.equal(result.status, "failed");
  assert.match(result.errorText, /simulation failed/);
  assert.equal(signed, false);
  assert.equal(sent, false);
});

test("direct sender surfaces local signing failures", async () => {
  const result = await sendDirectTransaction({
    connection: {
      simulateTransaction: () => ({ err: null }),
      sendRawTransaction: () => "signature-1"
    },
    signer: signer({
      signTransaction: () => {
        throw new Error("signer locked");
      }
    }),
    payload: payload(),
    config: { gate: liveGate }
  });

  assert.equal(result.status, "failed");
  assert.match(result.errorText, /signer locked/);
});

test("direct sender confirms successful direct submissions", async () => {
  const sent = [];
  const result = await sendDirectTransaction({
    connection: {
      getLatestBlockhash: () => ({ blockhash: "blockhash-1", lastValidBlockHeight: 123 }),
      simulateTransaction: () => ({ err: null, unitsConsumed: 42 }),
      sendRawTransaction: (serializedTransaction, options) => {
        sent.push({ serializedTransaction: [...serializedTransaction], options });
        return "signature-1";
      },
      confirmTransaction: (signature, blockhashContext) => {
        assert.equal(signature, "signature-1");
        assert.equal(blockhashContext.blockhash, "blockhash-1");
        return { err: null, slot: 55 };
      }
    },
    signer: signer(),
    payload: payload(),
    config: {
      gate: liveGate,
      skipPreflight: false,
      maxRetries: 3,
      nowMs: (() => {
        let now = 1000;
        return () => (now += 10);
      })()
    }
  });

  assert.equal(result.ok, true);
  assert.equal(result.status, "confirmed");
  assert.equal(result.signature, "signature-1");
  assert.equal(result.slot, 55);
  assert.equal(result.metadata.idempotencyKey, "chat:wallet:mint:buy");
  assert.deepEqual(sent[0].serializedTransaction, [1, 2, 3]);
  assert.deepEqual(sent[0].options, { skipPreflight: false, maxRetries: 3 });
});

test("Solana direct sender signs, simulates, sends, and confirms a versioned transaction payload", async () => {
  const solanaSigner = Keypair.generate();
  const payload = {
    provider: "direct-pump",
    route: {
      provider: "direct-pump",
      route: "pump-bonding-curve",
      mint: baseRequest.mint,
      walletPublicKey: solanaSigner.publicKey.toBase58(),
      poolAddress: null,
      priorityFeeSol: 0.00005,
      slippagePercent: 10,
      amount: "990000000",
      amountBasis: "sol"
    },
    instructions: [
      SystemProgram.transfer({
        fromPubkey: solanaSigner.publicKey,
        toPubkey: solanaSigner.publicKey,
        lamports: 0
      })
    ],
    signers: [],
    metadata: { idempotencyKey: "chat:wallet:mint:buy" }
  };
  const seen = {
    simulated: false,
    simulationOptions: null,
    serializedBytes: 0,
    confirmationSignature: null,
    stages: []
  };
  const result = await sendSolanaDirectTransaction({
    connection: {
      getLatestBlockhash: () => ({
        blockhash: "11111111111111111111111111111111",
        lastValidBlockHeight: 123
      }),
      simulateTransaction: (_transaction, options) => {
        seen.simulated = true;
        seen.simulationOptions = options;
        return { value: { err: null, unitsConsumed: 42 } };
      },
      sendRawTransaction: (serializedTransaction) => {
        seen.serializedBytes = serializedTransaction.length;
        return "signature-1";
      },
      confirmTransaction: ({ signature }) => {
        seen.confirmationSignature = signature;
        return { value: { err: null } };
      }
    },
    signer: solanaSigner,
    payload,
    config: {
      gate: liveGate,
      onStage: (stage, details) => {
        seen.stages.push({ stage, ...details });
      },
      nowMs: (() => {
        let now = 1000;
        return () => (now += 10);
      })()
    }
  });

  assert.equal(result.ok, true);
  assert.equal(result.status, "confirmed");
  assert.equal(result.signature, "signature-1");
  assert.equal(seen.simulated, true);
  assert.deepEqual(seen.simulationOptions, {
    replaceRecentBlockhash: true,
    sigVerify: false
  });
  assert.equal(seen.serializedBytes > 0, true);
  assert.equal(seen.confirmationSignature, "signature-1");
  assert.deepEqual(seen.stages.map((stage) => stage.stage), [
    "transaction_build_started",
    "blockhash_started",
    "blockhash_received",
    "signing_started",
    "signing_finished",
    "transaction_built",
    "simulation_started",
    "simulation_finished",
    "raw_send_started",
    "signature_returned",
    "confirmation_started",
    "confirmation_finished"
  ]);
  assert.equal(result.submittedAtMs, 1100);
  assert.equal(result.confirmedAtMs, 1120);
  assert.equal(result.metadata.directSolanaTiming.signatureReturnedAtMs, 1100);
  assert.equal(result.metadata.directSolanaTiming.confirmationFinishedAtMs, 1120);
  assert.equal(result.metadata.directSolanaTiming.timeToSignatureMs, 90);
  assert.equal(result.metadata.directSolanaTiming.confirmationMs, 10);
  assert.equal(result.metadata.directSolanaTiming.unitsConsumed, 42);
  assert.equal(result.metadata.directSolanaTiming.simulateBeforeSend, true);
});

test("Solana direct sender can skip explicit pre-send simulation", async () => {
  const solanaSigner = Keypair.generate();
  const payload = {
    provider: "direct-pump",
    route: {
      provider: "direct-pump",
      route: "pump-bonding-curve",
      mint: baseRequest.mint,
      walletPublicKey: solanaSigner.publicKey.toBase58(),
      poolAddress: null,
      priorityFeeSol: 0.00005,
      slippagePercent: 10,
      amount: "990000000",
      amountBasis: "sol"
    },
    instructions: [
      SystemProgram.transfer({
        fromPubkey: solanaSigner.publicKey,
        toPubkey: solanaSigner.publicKey,
        lamports: 0
      })
    ],
    signers: [],
    metadata: {}
  };
  let simulateCalls = 0;
  const stages = [];
  const result = await sendSolanaDirectTransaction({
    connection: {
      getLatestBlockhash: () => ({
        blockhash: "11111111111111111111111111111111",
        lastValidBlockHeight: 123
      }),
      simulateTransaction: () => {
        simulateCalls += 1;
        return { value: { err: null, unitsConsumed: 42 } };
      },
      sendRawTransaction: () => "signature-no-sim",
      confirmTransaction: () => ({ value: { err: null } })
    },
    signer: solanaSigner,
    payload,
    config: {
      gate: liveGate,
      simulateBeforeSend: false,
      onStage: (stage) => stages.push(stage)
    }
  });

  assert.equal(result.ok, true);
  assert.equal(result.signature, "signature-no-sim");
  assert.equal(simulateCalls, 0);
  assert.equal(result.metadata.directSolanaTiming.simulateBeforeSend, false);
  assert.equal(result.metadata.directSolanaTiming.unitsConsumed, null);
  assert.deepEqual(stages.includes("simulation_started"), false);
  assert.deepEqual(stages.includes("simulation_finished"), false);
});

test("Solana direct sender background mode returns after signature without confirmation wait", async () => {
  const solanaSigner = Keypair.generate();
  const payload = {
    provider: "direct-pump",
    route: {
      provider: "direct-pump",
      route: "pump-bonding-curve",
      mint: baseRequest.mint,
      walletPublicKey: solanaSigner.publicKey.toBase58(),
      poolAddress: null,
      priorityFeeSol: 0.00005,
      slippagePercent: 10,
      amount: "990000000",
      amountBasis: "sol"
    },
    instructions: [
      SystemProgram.transfer({
        fromPubkey: solanaSigner.publicKey,
        toPubkey: solanaSigner.publicKey,
        lamports: 0
      })
    ],
    signers: [],
    metadata: { idempotencyKey: "chat:wallet:mint:buy" }
  };
  let confirmationCalls = 0;
  const result = await sendSolanaDirectTransaction({
    connection: {
      getLatestBlockhash: () => ({
        blockhash: "11111111111111111111111111111111",
        lastValidBlockHeight: 123
      }),
      simulateTransaction: () => ({ value: { err: null, unitsConsumed: 42 } }),
      sendRawTransaction: () => "signature-background",
      confirmTransaction: () => {
        confirmationCalls += 1;
        throw new Error("confirmation should be backgrounded");
      }
    },
    signer: solanaSigner,
    payload,
    config: {
      gate: liveGate,
      confirmationMode: "background",
      nowMs: (() => {
        let now = 2000;
        return () => (now += 10);
      })()
    }
  });

  assert.equal(result.ok, true);
  assert.equal(result.status, "submitted");
  assert.equal(result.signature, "signature-background");
  assert.equal(result.submittedAtMs, 2100);
  assert.equal(result.confirmedAtMs, null);
  assert.equal(confirmationCalls, 0);
  assert.equal(result.metadata.confirmationMode, "background");
  assert.equal(result.metadata.directSolanaTiming.signatureReturnedAtMs, 2100);
  assert.equal(result.metadata.directSolanaTiming.confirmationStartedAtMs, null);
  assert.equal(result.metadata.directSolanaTiming.timeToSignatureMs, 90);
});

test("Solana direct sender reuses a warm blockhash cache", async () => {
  const solanaSigner = Keypair.generate();
  const payload = {
    provider: "direct-pump",
    route: {
      provider: "direct-pump",
      route: "pump-bonding-curve",
      mint: baseRequest.mint,
      walletPublicKey: solanaSigner.publicKey.toBase58(),
      poolAddress: null,
      priorityFeeSol: 0.00005,
      slippagePercent: 10,
      amount: "990000000",
      amountBasis: "sol"
    },
    instructions: [
      SystemProgram.transfer({
        fromPubkey: solanaSigner.publicKey,
        toPubkey: solanaSigner.publicKey,
        lamports: 0
      })
    ],
    signers: [],
    metadata: {}
  };
  let blockhashCalls = 0;
  let sendCalls = 0;
  const connection = {
    getLatestBlockhash: () => {
      blockhashCalls += 1;
      return {
        blockhash: "11111111111111111111111111111111",
        lastValidBlockHeight: 123
      };
    },
    simulateTransaction: () => ({ value: { err: null, unitsConsumed: 42 } }),
    sendRawTransaction: () => `signature-${++sendCalls}`,
    confirmTransaction: () => ({ value: { err: null } })
  };
  const seen = [];
  const config = {
    gate: liveGate,
    confirmationMode: "background",
    blockhashCacheMs: 30_000,
    onStage: (stage, details) => {
      if (stage === "blockhash_received") {
        seen.push(details.status);
      }
    }
  };

  const first = await sendSolanaDirectTransaction({ connection, signer: solanaSigner, payload, config });
  const second = await sendSolanaDirectTransaction({ connection, signer: solanaSigner, payload, config });

  assert.equal(first.signature, "signature-1");
  assert.equal(second.signature, "signature-2");
  assert.equal(blockhashCalls, 1);
  assert.deepEqual(seen, ["fresh", "cached"]);
  assert.equal(second.metadata.directSolanaTiming.blockhashCacheMs, 30_000);
});

test("Solana direct blockhash warmer can force refresh an unexpired cache", async () => {
  let blockhashCalls = 0;
  const connection = {
    getLatestBlockhash: () => {
      blockhashCalls += 1;
      return {
        blockhash: blockhashCalls === 1
          ? "11111111111111111111111111111111"
          : "22222222222222222222222222222222",
        lastValidBlockHeight: 123 + blockhashCalls
      };
    }
  };

  await warmDirectSolanaBlockhash({ connection, cacheMs: 30_000 });
  await warmDirectSolanaBlockhash({ connection, cacheMs: 30_000 });
  assert.equal(blockhashCalls, 1);

  await warmDirectSolanaBlockhash({ connection, cacheMs: 30_000, forceRefresh: true });
  assert.equal(blockhashCalls, 2);
});

test("Solana direct sender can fan out raw sends and returns the first RPC signature", async () => {
  const solanaSigner = Keypair.generate();
  const payload = {
    provider: "direct-pump",
    route: {
      provider: "direct-pump",
      route: "pump-bonding-curve",
      mint: baseRequest.mint,
      walletPublicKey: solanaSigner.publicKey.toBase58(),
      poolAddress: null,
      priorityFeeSol: 0.00005,
      slippagePercent: 10,
      amount: "990000000",
      amountBasis: "sol"
    },
    instructions: [
      SystemProgram.transfer({
        fromPubkey: solanaSigner.publicKey,
        toPubkey: solanaSigner.publicKey,
        lamports: 0
      })
    ],
    signers: [],
    metadata: {}
  };
  const stages = [];
  const result = await sendSolanaDirectTransaction({
    connection: {
      getLatestBlockhash: () => ({
        blockhash: "11111111111111111111111111111111",
        lastValidBlockHeight: 123
      }),
      simulateTransaction: () => ({ value: { err: null, unitsConsumed: 42 } }),
      sendRawTransaction: () => new Promise((resolve) => setTimeout(() => resolve("signature-primary"), 20)),
      confirmTransaction: () => ({ value: { err: null } })
    },
    signer: solanaSigner,
    payload,
    config: {
      gate: liveGate,
      confirmationMode: "background",
      simulateBeforeSend: false,
      sendConnections: [
        {
          label: "primary",
          url: "https://primary.example",
          connection: {
            sendRawTransaction: () => new Promise((resolve) => setTimeout(() => resolve("signature-primary"), 20))
          }
        },
        {
          label: "jito-1",
          url: "https://jito.example/api/v1/transactions",
          connection: {
            sendRawTransaction: () => "signature-fanout"
          }
        }
      ],
      onStage: (stage, details) => stages.push({ stage, ...details })
    }
  });

  assert.equal(result.ok, true);
  assert.equal(result.signature, "signature-fanout");
  assert.equal(result.metadata.directSolanaTiming.rawSendRpcCount, 2);
  assert.equal(result.metadata.directSolanaTiming.rawSendWinner, "jito-1");
  assert.equal(result.metadata.directSolanaTiming.rawSendJitoEnabled, true);
  assert.equal(result.metadata.directSolanaTiming.rawSendJitoRpcCount, 1);
  assert.equal(result.metadata.directSolanaTiming.rawSendJitoWinner, true);
  assert.equal(stages.find((stage) => stage.stage === "raw_send_started").rpcCount, 2);
  assert.equal(stages.find((stage) => stage.stage === "raw_send_started").jitoRpcCount, 1);
});

test("Solana direct send connection builder dedupes primary and labels fanout/Jito RPCs", async () => {
  const primaryConnection = { sendRawTransaction: () => "signature-primary" };
  const connections = buildDirectSolanaSendConnections({
    primaryConnection,
    primaryUrl: "https://primary.example",
    urls: ["https://primary.example", "https://fanout.example"],
    jitoUrls: ["https://frankfurt.mainnet.block-engine.jito.wtf"],
    jitoAuthUuid: "uuid-for-test"
  });

  assert.equal(connections.length, 3);
  assert.equal(connections[0].label, "primary");
  assert.equal(connections[0].connection, primaryConnection);
  assert.equal(connections[1].label, "fanout-1");
  assert.equal(connections[1].url, "https://fanout.example");
  assert.equal(connections[2].label, "jito-1");
  assert.equal(connections[2].url, "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/transactions");

  const originalFetch = globalThis.fetch;
  const requests = [];
  globalThis.fetch = async (url, init) => {
    requests.push({ url, init });
    return new Response(JSON.stringify({ jsonrpc: "2.0", id: 1, result: "signature-jito" }), {
      status: 200,
      headers: { "content-type": "application/json" }
    });
  };
  try {
    const signature = await connections[2].connection.sendRawTransaction(Buffer.from("tx"), {
      skipPreflight: true,
      maxRetries: 0
    });
    assert.equal(signature, "signature-jito");
  } finally {
    globalThis.fetch = originalFetch;
  }

  assert.equal(requests.length, 1);
  assert.equal(requests[0].url, "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/transactions");
  assert.equal(requests[0].init.headers["x-jito-auth"], "uuid-for-test");
  const body = JSON.parse(requests[0].init.body);
  assert.equal(body.method, "sendTransaction");
  assert.equal(body.params[1].encoding, "base64");
  assert.equal(body.params[1].skipPreflight, true);
  assert.equal(body.params[1].maxRetries, 0);
});

test("Solana direct sender clamps excessive blockhash cache windows", async () => {
  const solanaSigner = Keypair.generate();
  const payload = {
    provider: "direct-pump",
    route: {
      provider: "direct-pump",
      route: "pump-bonding-curve",
      mint: baseRequest.mint,
      walletPublicKey: solanaSigner.publicKey.toBase58(),
      poolAddress: null,
      priorityFeeSol: 0.00005,
      slippagePercent: 10,
      amount: "990000000",
      amountBasis: "sol"
    },
    instructions: [
      SystemProgram.transfer({
        fromPubkey: solanaSigner.publicKey,
        toPubkey: solanaSigner.publicKey,
        lamports: 0
      })
    ],
    signers: [],
    metadata: {}
  };
  let blockhashCalls = 0;
  const connection = {
    getLatestBlockhash: () => {
      blockhashCalls += 1;
      return {
        blockhash: "11111111111111111111111111111111",
        lastValidBlockHeight: 123
      };
    },
    simulateTransaction: () => ({ value: { err: null, unitsConsumed: 42 } }),
    sendRawTransaction: () => "signature-clamped",
    confirmTransaction: () => ({ value: { err: null } })
  };
  const result = await sendSolanaDirectTransaction({
    connection,
    signer: solanaSigner,
    payload,
    config: {
      gate: liveGate,
      confirmationMode: "background",
      blockhashCacheMs: 999_999
    }
  });

  assert.equal(result.ok, true);
  assert.equal(blockhashCalls, 1);
  assert.equal(result.metadata.directSolanaTiming.blockhashCacheMs, 30_000);
});

test("Solana direct builder resolves legacy and Token-2022 mint programs", async () => {
  const mint = Keypair.generate().publicKey;

  const legacyProgram = await resolveMintTokenProgram({
    connection: {
      getAccountInfo: () => ({ owner: TOKEN_PROGRAM_ID })
    },
    mint
  });
  assert.equal(legacyProgram.toBase58(), TOKEN_PROGRAM_ID.toBase58());

  const token2022Program = await resolveMintTokenProgram({
    connection: {
      getAccountInfo: () => ({ owner: TOKEN_2022_PROGRAM_ID })
    },
    mint
  });
  assert.equal(token2022Program.toBase58(), TOKEN_2022_PROGRAM_ID.toBase58());
});

test("Solana direct builder caches mint token program resolution per connection", async () => {
  const mint = Keypair.generate().publicKey;
  let accountInfoCalls = 0;
  const connection = {
    getAccountInfo: () => {
      accountInfoCalls += 1;
      return { owner: TOKEN_PROGRAM_ID };
    }
  };

  const first = await resolveMintTokenProgram({ connection, mint });
  const second = await resolveMintTokenProgram({ connection, mint });

  assert.equal(first.toBase58(), TOKEN_PROGRAM_ID.toBase58());
  assert.equal(second.toBase58(), TOKEN_PROGRAM_ID.toBase58());
  assert.equal(accountInfoCalls, 1);
});

test("Solana direct Pump buy state batches mint, bonding curve, and both ATA candidates", async () => {
  const mint = Keypair.generate().publicKey;
  const user = Keypair.generate().publicKey;
  const bondingCurve = Keypair.generate().publicKey;
  const legacyAtaInfo = { owner: TOKEN_PROGRAM_ID, data: Buffer.alloc(0) };
  const token2022AtaInfo = { owner: TOKEN_2022_PROGRAM_ID, data: Buffer.alloc(0) };
  const seen = [];

  const result = await fetchDirectPumpBuyState({
    connection: {
      getMultipleAccountsInfo: (accounts) => {
        seen.push(accounts.map((account) => account.toBase58()));
        return [
          { owner: TOKEN_2022_PROGRAM_ID, data: Buffer.alloc(0) },
          { owner: SystemProgram.programId, data: Buffer.alloc(8) },
          legacyAtaInfo,
          token2022AtaInfo
        ];
      }
    },
    pumpModule: {
      bondingCurvePda: () => bondingCurve,
      PUMP_SDK: {
        decodeBondingCurve: () => ({
          complete: false,
          tokenTotalSupply: 1n
        })
      }
    },
    mint,
    user
  });

  assert.equal(seen.length, 1);
  assert.equal(seen[0].length, 4);
  assert.equal(seen[0][0], mint.toBase58());
  assert.equal(seen[0][1], bondingCurve.toBase58());
  assert.equal(result.tokenProgram.toBase58(), TOKEN_2022_PROGRAM_ID.toBase58());
  assert.equal(result.associatedUserAccountInfo, token2022AtaInfo);
});

test("Solana direct Pump fast buy state cache fails closed and refreshes known reserves", () => {
  const mint = Keypair.generate().publicKey.toBase58();
  const creator = Keypair.generate().publicKey.toBase58();

  assert.equal(primeDirectPumpFastBuyState({
    mint,
    creator,
    tokenProgram: SystemProgram.programId.toBase58(),
    virtualTokenReserves: "1073000000000000",
    virtualQuoteReserves: "30000000000",
    realTokenReserves: "793000000000000",
    realQuoteReserves: "0",
    tokenTotalSupply: "1000000000000000"
  }), false);

  assert.equal(refreshDirectPumpFastBuyStateReserves({
    mint,
    virtualTokenReserves: "1072000000000000",
    virtualQuoteReserves: "30100000000"
  }), false);

  assert.equal(primeDirectPumpFastBuyState({
    mint,
    creator,
    tokenProgram: TOKEN_2022_PROGRAM_ID.toBase58(),
    virtualTokenReserves: "1073000000000000",
    virtualQuoteReserves: "30000000000",
    realTokenReserves: "793000000000000",
    realQuoteReserves: "0",
    tokenTotalSupply: "1000000000000000"
  }), true);

  assert.equal(refreshDirectPumpFastBuyStateReserves({
    mint,
    virtualTokenReserves: "1072000000000000",
    virtualQuoteReserves: "30100000000"
  }), true);
});

test("Solana direct Pump extracts observed BuyV2 creator vault from inner instructions", () => {
  const mint = Keypair.generate().publicKey;
  const creatorVault = Keypair.generate().publicKey;
  const pumpProgram = new PublicKey("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
  const accountKeys = Array.from({ length: 25 }, () => Keypair.generate().publicKey);
  accountKeys[1] = mint;
  accountKeys[16] = creatorVault;
  accountKeys[24] = pumpProgram;
  const accounts = Array.from({ length: 18 }, (_, index) => index);

  const extracted = extractPumpBuyV2CreatorVaultFromTransaction({
    transaction: {
      message: {
        staticAccountKeys: accountKeys,
        compiledInstructions: []
      }
    },
    meta: {
      innerInstructions: [
        {
          instructions: [
            {
              programIdIndex: 24,
              accounts
            }
          ]
        }
      ]
    }
  }, mint);

  assert.equal(extracted?.toBase58(), creatorVault.toBase58());
});

test("Solana direct builder rejects missing or non-token mint accounts", async () => {
  const mint = Keypair.generate().publicKey;

  await assert.rejects(
    resolveMintTokenProgram({
      connection: {
        getAccountInfo: () => null
      },
      mint
    }),
    /mint account not found/
  );

  await assert.rejects(
    resolveMintTokenProgram({
      connection: {
        getAccountInfo: () => ({ owner: SystemProgram.programId })
      },
      mint
    }),
    /not owned by SPL Token or Token-2022/
  );
});

test("direct sender preserves signature when confirmation lookup throws after send", async () => {
  const result = await sendDirectTransaction({
    connection: {
      getLatestBlockhash: () => ({ blockhash: "blockhash-1", lastValidBlockHeight: 123 }),
      simulateTransaction: () => ({ err: null }),
      sendRawTransaction: () => "signature-submitted",
      confirmTransaction: () => {
        throw new Error("confirmation rpc timeout");
      }
    },
    signer: signer(),
    payload: payload(),
    config: { gate: liveGate }
  });

  assert.equal(result.ok, true);
  assert.equal(result.status, "submitted");
  assert.equal(result.signature, "signature-submitted");
  assert.equal(result.metadata.confirmationError, "confirmation rpc timeout");
});

test("direct sender surfaces send and confirmation failures", async () => {
  const sendFail = await sendDirectTransaction({
    connection: {
      simulateTransaction: () => ({ err: null }),
      sendRawTransaction: () => {
        throw new Error("rpc send failed");
      }
    },
    signer: signer(),
    payload: payload(),
    config: { gate: liveGate }
  });

  assert.equal(sendFail.status, "failed");
  assert.match(sendFail.errorText, /rpc send failed/);

  const confirmFail = await sendDirectTransaction({
    connection: {
      simulateTransaction: () => ({ err: null }),
      sendRawTransaction: () => "signature-2",
      confirmTransaction: () => ({ err: "not finalized", slot: 66 })
    },
    signer: signer(),
    payload: payload(),
    config: { gate: liveGate }
  });

  assert.equal(confirmFail.status, "failed");
  assert.equal(confirmFail.signature, "signature-2");
  assert.match(confirmFail.errorText, /not finalized/);
  assert.equal(confirmFail.slot, 66);
});

test("direct quote cap keeps SDK slippage-inclusive max spend within the trade budget", () => {
  const tradeBudgetLamports = 990_000_000n;
  const quoteLamports = maxQuoteLamportsForSlippageCap(tradeBudgetLamports, 10);
  const slippageInclusiveMax = (quoteLamports * 1_100_000_000n) / 1_000_000_000n;

  assert.equal(quoteLamports, 900_000_000n);
  assert.equal(slippageInclusiveMax <= tradeBudgetLamports, true);
  assert.equal(maxQuoteLamportsForSlippageCap(tradeBudgetLamports, 0), tradeBudgetLamports);
});

test("direct sender emergency stop blocks even otherwise-live direct config", async () => {
  const result = await sendDirectTransaction({
    connection: {},
    signer: signer(),
    payload: payload(),
    config: {
      gate: {
        ...liveGate,
        emergencyStopped: true
      }
    }
  });

  assert.equal(result.status, "skipped");
  assert.match(result.errorText, /emergency stop/);
});
