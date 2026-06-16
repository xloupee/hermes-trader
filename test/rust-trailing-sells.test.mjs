import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("Rust copy buy executions are bridged into confirmation-gated trailing sells", async () => {
  const [indexSource, typesSource, envExample, readme] = await Promise.all([
    readFile("src/index.ts", "utf8"),
    readFile("src/types.ts", "utf8"),
    readFile(".env.example", "utf8"),
    readFile("README.md", "utf8")
  ]);

  assert.match(typesSource, /copyTradeRustTrailingSellsEnabled: boolean;/);
  assert.match(typesSource, /copyTradeRustTrailingSellsSource: "supabase" \| "local-jsonl";/);
  assert.match(typesSource, /copyTradeRustTrailingSellsExecutionProvider:/);
  assert.match(indexSource, /COPY_TRADE_RUST_TRAILING_SELLS_ENABLED/);
  assert.match(indexSource, /COPY_TRADE_RUST_TRAILING_SELLS_LIVE_ENABLED/);
  assert.match(indexSource, /COPY_TRADE_RUST_TRAILING_SELLS_SOURCE/);
  assert.match(indexSource, /COPY_TRADE_RUST_TRAILING_SELLS_LOCAL_EXECUTIONS_PATH/);
  assert.match(indexSource, /COPY_TRADE_RUST_TRAILING_SELLS_CONFIRMATION_POLL_MS/);
  assert.match(envExample, /COPY_TRADE_RUST_EXECUTION_ALERTS_SOURCE=local-jsonl/);
  assert.match(envExample, /COPY_TRADE_RUST_EXECUTION_ALERTS_LOCAL_EXECUTIONS_PATH=\/var\/log\/jito-copy-executions-vps\.jsonl/);
  assert.match(readme, /COPY_TRADE_RUST_EXECUTION_ALERTS_SOURCE=local-jsonl/);

  assert.match(
    indexSource,
    /function rustTrailingSellSourceFromEnv[\s\S]*"supabase" \? "supabase" : "local-jsonl"/
  );
  assert.match(
    indexSource,
    /fetchRecentRustCopyBuyExecutionRows[\s\S]*copyTradeRustTrailingSellsSource === "local-jsonl"[\s\S]*fetchNewRustCopyBuyExecutionRowsFromLocalFile/
  );
  assert.match(
    indexSource,
    /fetchNewRustCopyBuyExecutionRowsFromLocalFile[\s\S]*rustTrailingSellLocalFileOffset[\s\S]*JSON\.parse\(line\)/
  );
  assert.match(
    indexSource,
    /normalizeRustCopyBuyExecutionRow[\s\S]*schema !== "copytrade\.localExecution\.v1"[\s\S]*decision !== "sent"[\s\S]*!sent/
  );

  assert.match(
    indexSource,
    /fetchRecentRustCopyBuyExecutionRows[\s\S]*provider", "eq\.shredstream"[\s\S]*observed_action", "eq\.buy"[\s\S]*send_signature", "not\.is\.null"/
  );
  assert.match(
    indexSource,
    /activeCopyTradeEntriesForTarget\(row\.observed_wallet\)[\s\S]*entry\.subscriber\.tradingWallet\?\.publicKey === row\.copy_wallet/
  );
  assert.match(
    indexSource,
    /scheduleRustTrailingSellsForExecution[\s\S]*submissionBlockedReason: rustTrailingSellSubmissionBlockedReason[\s\S]*includeBuyPressureSell: false[\s\S]*confirmationPollMs: config\.copyTradeRustTrailingSellsConfirmationPollMs[\s\S]*scheduleAnchorAtMs: rustCopyBuyScheduleAnchorAtMs\(row\)[\s\S]*scheduleCopyTradeTrailingSellsAfterConfirmation\([\s\S]*buySignature: row\.send_signature/
  );
  assert.match(
    indexSource,
    /function rustCopyBuyScheduleAnchorAtMs[\s\S]*signatureReturnedAtMs[\s\S]*sendSubmittedAtMs/
  );
  assert.match(
    indexSource,
    /const startedAt = trailingSellContext\.scheduleAnchorAtMs \?\? Date\.now\(\);/
  );
  assert.match(
    indexSource,
    /Rust trailing sell step due/
  );
  assert.match(
    indexSource,
    /for \(const row of rows\) \{[\s\S]*scheduleRustTrailingSellsForExecution\(row\)\.catch/
  );
  assert.match(
    indexSource,
    /function scheduleCopyTradeTrailingSellsAfterConfirmation[\s\S]*waitForSignatureConfirmationResult\([\s\S]*buySignature,[\s\S]*30000,[\s\S]*trailingSellContext\.confirmationPollMs/
  );
  assert.match(
    indexSource,
    /executeDirectCopyTrade[\s\S]*directGate\?: DirectExecutionGateConfig[\s\S]*gate: directGate \|\| directExecutionGate\(provider\)/
  );
});
