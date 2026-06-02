import assert from "node:assert/strict";
import test from "node:test";
import { TOKEN_2022_PROGRAM_ID } from "@solana/spl-token";
import {
  buildDirectRouteMetadata,
  directExecutionLiveBlockedReason,
  directExecutionLiveEnabled,
  formatTradeExecutionResultLog,
  isDirectTradeExecutionProvider,
  parseTradeExecutionProvider,
  tradeExecutionProviderConfigError,
  tradeExecutionSkippedResult
} from "../dist/trade-execution.js";

test("trade execution provider parsing defaults to direct auto execution", () => {
  assert.equal(parseTradeExecutionProvider(null), "direct-auto");
  assert.equal(parseTradeExecutionProvider(" DIRECT-PUMP "), "direct-pump");
  assert.equal(parseTradeExecutionProvider("nope"), "direct-auto");
  assert.equal(tradeExecutionProviderConfigError("nope")?.includes("unsupported trade execution provider"), true);
  assert.equal(tradeExecutionProviderConfigError("direct-pumpswap"), null);
});

test("trade execution helpers identify only direct providers as direct", () => {
  assert.equal(isDirectTradeExecutionProvider("pumpportal-lightning"), false);
  assert.equal(isDirectTradeExecutionProvider("direct-pump"), true);
  assert.equal(isDirectTradeExecutionProvider("direct-pumpswap"), true);
  assert.equal(isDirectTradeExecutionProvider("direct-auto"), true);
});

test("direct execution live gate fails closed until every direct/live gate is explicit", () => {
  const base = {
    provider: "direct-pump",
    copyTradeEnabled: true,
    copyTradeDryRun: false,
    directExecutionEnabled: true,
    directExecutionLiveEnabled: true
  };

  assert.equal(directExecutionLiveEnabled(base), true);
  assert.equal(directExecutionLiveBlockedReason(base), null);

  assert.match(
    directExecutionLiveBlockedReason({ ...base, provider: "pumpportal-lightning" }),
    /not a direct provider/
  );
  assert.equal(
    directExecutionLiveBlockedReason({ ...base, copyTradeEnabled: false }),
    "COPY_TRADE_ENABLED is not true"
  );
  assert.equal(
    directExecutionLiveBlockedReason({ ...base, copyTradeDryRun: true }),
    "COPY_TRADE_DRY_RUN is enabled"
  );
  assert.equal(
    directExecutionLiveBlockedReason({ ...base, directExecutionEnabled: false }),
    "DIRECT_EXECUTION_ENABLED is not true"
  );
  assert.equal(
    directExecutionLiveBlockedReason({ ...base, directExecutionLiveEnabled: false }),
    "DIRECT_EXECUTION_LIVE_ENABLED is not true"
  );
  assert.equal(
    directExecutionLiveBlockedReason({ ...base, directExecutionBuildOnly: true }),
    "direct execution build-only mode is enabled"
  );
  assert.equal(
    directExecutionLiveBlockedReason({ ...base, directExecutionSimulateOnly: true }),
    "direct execution simulate-only mode is enabled"
  );
  assert.equal(
    directExecutionLiveBlockedReason({ ...base, emergencyStopped: true }),
    "copy trade emergency stop is active"
  );
});

test("direct route metadata records route-specific execution context", () => {
  assert.deepEqual(
    buildDirectRouteMetadata({
      provider: "direct-pumpswap",
      mint: "Mint111111111111111111111111111111111111111",
      walletPublicKey: "Wallet111111111111111111111111111111111111",
      poolAddress: "Pool1111111111111111111111111111111111111",
      priorityFeeSol: 0.00005,
      slippagePercent: 10,
      amount: 1000000,
      amountBasis: "sol"
    }),
    {
      provider: "direct-pumpswap",
      route: "pumpswap-amm",
      mint: "Mint111111111111111111111111111111111111111",
      walletPublicKey: "Wallet111111111111111111111111111111111111",
      poolAddress: "Pool1111111111111111111111111111111111111",
      priorityFeeSol: 0.00005,
      slippagePercent: 10,
      amount: 1000000,
      amountBasis: "sol"
    }
  );
});

test("provider-neutral result log includes platform fee accounting", () => {
  const result = tradeExecutionSkippedResult({
    provider: "direct-pump",
    route: "pump-bonding-curve",
    reason: "build-only",
    platformFee: {
      enabled: true,
      bps: 100,
      treasury: "Treasury111111111111111111111111111111111",
      budgetLamports: 1000n,
      feeLamports: 10n,
      tradeLamports: 990n
    }
  });

  assert.match(formatTradeExecutionResultLog(result), /provider=direct-pump/);
  assert.match(formatTradeExecutionResultLog(result), /platformFeeBps=100/);
  assert.match(formatTradeExecutionResultLog(result), /tradeLamports=990/);
  assert.match(formatTradeExecutionResultLog(result), /budgetLamports=1000/);
});

test("provider-neutral result log includes direct Pump buy-state timing", () => {
  const result = tradeExecutionSkippedResult({
    provider: "direct-pump",
    route: "pump-bonding-curve",
    reason: "build-only",
    metadata: {
      directBuildTiming: {
        totalMs: 42,
        stages: [
          {
            stage: "buy_accounts_ready",
            source: "rpc",
            cachedStateSource: "rpc-bonding-curve-prefetch",
            cachedStateAgeMs: 12,
            creatorSource: "rpc-bonding-curve-prefetch:fanout-1",
            creatorVerifiedAgeMs: 24,
            tokenProgram: TOKEN_2022_PROGRAM_ID.toBase58(),
            forceFreshBuyState: true
          },
          {
            stage: "instructions_ready",
            buyInstructionBuilder: "local"
          }
        ]
      }
    }
  });

  const log = formatTradeExecutionResultLog(result);
  assert.match(log, /directBuildMs=42/);
  assert.match(log, /directBuyBuilder=local/);
  assert.match(log, /directBuyState=rpc/);
  assert.match(log, /directBuyStateSource=rpc-bonding-curve-prefetch/);
  assert.match(log, /directBuyStateAgeMs=12/);
  assert.match(log, /directCreatorSource=rpc-bonding-curve-prefetch:fanout-1/);
  assert.match(log, /directCreatorAgeMs=24/);
  assert.match(log, new RegExp(`tokenProgram=${TOKEN_2022_PROGRAM_ID.toBase58()}`));
  assert.match(log, /forceFreshBuyState=true/);
});

test("provider-neutral result log includes raw send fanout timing", () => {
  const result = tradeExecutionSkippedResult({
    provider: "direct-pump",
    route: "pump-bonding-curve",
    reason: "build-only",
    metadata: {
      directSolanaTiming: {
        timeToSignatureMs: 88,
        rawSendMs: 17,
        rawSendWinner: "jito-1",
        rawSendRpcCount: 4,
        rawSendJitoEnabled: true,
        rawSendJitoRpcCount: 1,
        rawSendJitoWinner: true
      }
    }
  });

  const log = formatTradeExecutionResultLog(result);
  assert.match(log, /timeToSignatureMs=88/);
  assert.match(log, /rawSendMs=17/);
  assert.match(log, /rawSendWinner=jito-1/);
  assert.match(log, /rawSendRpcCount=4/);
  assert.match(log, /jitoSend=enabled/);
  assert.match(log, /jitoWinner=true/);
});

test("provider-neutral result log includes PumpSwap build timing", () => {
  const result = tradeExecutionSkippedResult({
    provider: "direct-pumpswap",
    route: "pumpswap-amm",
    reason: "build-only",
    metadata: {
      directBuildTiming: {
        totalMs: 123,
        stages: [
          {
            stage: "swap_state_ready",
            durationMs: 87,
            source: "cache",
            cachedStateSource: "shredstream-observed-buy-prefetch",
            cachedStateAgeMs: 12
          },
          {
            stage: "quote_ready",
            durationMs: 3
          },
          {
            stage: "instructions_ready",
            durationMs: 14,
            buyInstructionBuilder: "local-pump-amm"
          }
        ]
      }
    }
  });

  const log = formatTradeExecutionResultLog(result);
  assert.match(log, /directBuildMs=123/);
  assert.match(log, /directPumpSwapStateMs=87/);
  assert.match(log, /directPumpSwapState=cache/);
  assert.match(log, /directPumpSwapStateSource=shredstream-observed-buy-prefetch/);
  assert.match(log, /directPumpSwapStateAgeMs=12/);
  assert.match(log, /directQuoteMs=3/);
  assert.match(log, /directBuyBuilder=local-pump-amm/);
  assert.match(log, /directInstructionsMs=14/);
});
