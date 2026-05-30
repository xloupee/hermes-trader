import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  copyTradeBuyIdempotencyKey,
  createJsonCopyTradeBuyIdempotencyStore,
  createSupabaseCopyTradeBuyIdempotencyStore
} from "../dist/copytrade-idempotency.js";

const request = {
  action: "buy",
  mint: "Mint111111111111111111111111111111111111111",
  amount: 0.1,
  denominatedInSol: "true",
  slippage: 10,
  priorityFee: 0.00005,
  pool: "pump"
};

function claimInput(overrides = {}) {
  const input = {
    chatId: "chat-1",
    sourceWalletAddress: "SourceWallet11111111111111111111111111111111",
    tradingWalletPublicKey: "TradingWallet1111111111111111111111111111111",
    observedSignature: "target-buy-signature-1",
    mint: request.mint,
    action: "buy",
    amountSol: 0.1,
    provider: "helius",
    request,
    now: "2026-05-24T12:00:00.000Z",
    ...overrides
  };

  return {
    ...input,
    key: overrides.key || copyTradeBuyIdempotencyKey(input)
  };
}

async function tempStorePath(t) {
  const dir = await mkdtemp(join(tmpdir(), "copytrade-idempotency-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  return join(dir, "copytrade-buy-idempotency.json");
}

function snakeCaseClaimRow(input, status = "claimed") {
  const now = input.now || "2026-05-24T12:00:00.000Z";

  return {
    idempotency_key: input.key,
    chat_id: input.chatId,
    source_wallet_address: input.sourceWalletAddress,
    trading_wallet_public_key: input.tradingWalletPublicKey,
    observed_signature: input.observedSignature,
    mint: input.mint,
    action: input.action || "buy",
    amount_sol: input.amountSol,
    provider: input.provider,
    request: input.request,
    status,
    result_signature: null,
    error_text: status === "failed" ? "HTTP 500" : null,
    http_status: null,
    response: null,
    claimed_at: now,
    updated_at: now,
    completed_at: status === "failed" ? now : null
  };
}

function createFakeSupabaseIdempotencyClient(seedRows = []) {
  const rows = seedRows.map((row) => ({ ...row }));
  const matches = (row, filters) => filters.every(({ column, value }) => row[column] === value);

  return {
    rows,
    client: {
      from(table) {
        assert.equal(table, "telegram_copytrade_buy_idempotency");

        return {
          insert(row) {
            const duplicate = rows.some((entry) =>
              entry.idempotency_key === row.idempotency_key ||
              (
                entry.chat_id === row.chat_id &&
                entry.mint === row.mint &&
                entry.action === row.action
              )
            );

            if (duplicate) {
              return Promise.resolve({
                error: {
                  code: "23505",
                  message: "duplicate key value violates unique constraint"
                }
              });
            }

            rows.push({ ...row });
            return Promise.resolve({ error: null });
          },
          select() {
            const filters = [];

            return {
              eq(column, value) {
                filters.push({ column, value });
                return this;
              },
              order() {
                return this;
              },
              limit() {
                return this;
              },
              maybeSingle() {
                const row = rows.filter((entry) => matches(entry, filters))[0] || null;
                return Promise.resolve({ data: row ? { ...row } : null, error: null });
              }
            };
          },
          update(values) {
            const filters = [];
            let wantsRow = false;

            const query = {
              eq(column, value) {
                filters.push({ column, value });
                return this;
              },
              select() {
                wantsRow = true;
                return this;
              },
              maybeSingle() {
                const index = rows.findIndex((entry) => matches(entry, filters));

                if (index === -1) {
                  return Promise.resolve({ data: null, error: null });
                }

                rows[index] = { ...rows[index], ...values };
                return Promise.resolve({
                  data: wantsRow ? { ...rows[index] } : null,
                  error: null
                });
              }
            };

            return query;
          }
        };
      }
    }
  };
}

function createMissingSupabaseIdempotencyClient() {
  const missing = {
    code: "PGRST205",
    message: "Could not find the table 'public.telegram_copytrade_buy_idempotency' in the schema cache"
  };

  return {
    from() {
      return {
        insert() {
          return Promise.resolve({ error: missing });
        },
        select() {
          return {
            eq() {
              return this;
            },
            order() {
              return this;
            },
            limit() {
              return this;
            },
            maybeSingle() {
              return Promise.resolve({ data: null, error: missing });
            }
          };
        },
        update() {
          return {
            eq() {
              return this;
            },
            select() {
              return this;
            },
            maybeSingle() {
              return Promise.resolve({ data: null, error: missing });
            }
          };
        }
      };
    }
  };
}

function createProviderConstraintSupabaseIdempotencyClient() {
  const providerCheck = {
    code: "23514",
    message: 'new row for relation "telegram_copytrade_buy_idempotency" violates check constraint "telegram_copytrade_buy_idempotency_provider_check"'
  };

  return {
    from() {
      return {
        insert() {
          return Promise.resolve({ error: providerCheck });
        },
        select() {
          return {
            eq() {
              return this;
            },
            order() {
              return this;
            },
            limit() {
              return this;
            },
            maybeSingle() {
              return Promise.resolve({ data: null, error: null });
            }
          };
        },
        update() {
          return {
            eq() {
              return this;
            },
            select() {
              return this;
            },
            maybeSingle() {
              return Promise.resolve({ data: null, error: null });
            }
          };
        }
      };
    }
  };
}

test("JSON copy buy idempotency blocks duplicate claims before submit", async (t) => {
  const path = await tempStorePath(t);
  const store = createJsonCopyTradeBuyIdempotencyStore({ path });

  const first = await store.claimBuy(claimInput());
  const second = await store.claimBuy(claimInput());

  assert.equal(first.claimed, true);
  assert.equal(second.claimed, false);
  assert.equal(second.existing?.status, "claimed");
  assert.equal(second.existing?.observedSignature, "target-buy-signature-1");
});

test("JSON copy buy idempotency preserves Geyser provider records", async (t) => {
  const path = await tempStorePath(t);
  const store = createJsonCopyTradeBuyIdempotencyStore({ path });
  const input = claimInput({ provider: "geyser" });

  assert.equal((await store.claimBuy(input)).claimed, true);

  const restartedStore = createJsonCopyTradeBuyIdempotencyStore({ path });
  const duplicate = await restartedStore.claimBuy(input);

  assert.equal(duplicate.claimed, false);
  assert.equal(duplicate.existing?.provider, "geyser");
});

test("JSON copy buy idempotency survives restart after completion", async (t) => {
  const path = await tempStorePath(t);
  const firstStore = createJsonCopyTradeBuyIdempotencyStore({ path });

  assert.equal((await firstStore.claimBuy(claimInput())).claimed, true);
  await firstStore.completeBuy(claimInput().key, {
    ok: true,
    status: 200,
    signature: "copy-buy-signature-1",
    errorText: null,
    raw: { signature: "copy-buy-signature-1" }
  });

  const restartedStore = createJsonCopyTradeBuyIdempotencyStore({ path });
  const duplicate = await restartedStore.claimBuy(claimInput());
  const stored = JSON.parse(await readFile(path, "utf8"));

  assert.equal(duplicate.claimed, false);
  assert.equal(duplicate.existing?.status, "submitted");
  assert.equal(duplicate.existing?.resultSignature, "copy-buy-signature-1");
  assert.equal(stored.records.length, 1);
});

test("JSON copy buy idempotency blocks same mint after failed submission", async (t) => {
  const path = await tempStorePath(t);
  const firstStore = createJsonCopyTradeBuyIdempotencyStore({ path });

  assert.equal((await firstStore.claimBuy(claimInput())).claimed, true);
  await firstStore.failBuy(claimInput().key, "HTTP 500");

  const restartedStore = createJsonCopyTradeBuyIdempotencyStore({ path });
  const duplicate = await restartedStore.claimBuy(claimInput({
    observedSignature: "target-buy-signature-2"
  }));

  assert.equal(duplicate.claimed, false);
  assert.equal(duplicate.existing?.status, "failed");
  assert.equal(duplicate.existing?.errorText, "HTTP 500");
});

test("JSON copy buy idempotency retries failed same mint when enabled", async (t) => {
  const path = await tempStorePath(t);
  const firstStore = createJsonCopyTradeBuyIdempotencyStore({ path });

  assert.equal((await firstStore.claimBuy(claimInput())).claimed, true);
  await firstStore.failBuy(claimInput().key, "HTTP 500");

  const restartedStore = createJsonCopyTradeBuyIdempotencyStore({ path });
  const retry = await restartedStore.claimBuy(claimInput({
    observedSignature: "target-buy-signature-2",
    sourceWalletAddress: "RetrySourceWallet11111111111111111111111111",
    retryFailed: true,
    now: "2026-05-24T12:05:00.000Z"
  }));
  const stored = JSON.parse(await readFile(path, "utf8"));

  assert.equal(retry.claimed, true);
  assert.equal(retry.existing, null);
  assert.equal(stored.records.length, 1);
  assert.equal(stored.records[0].status, "claimed");
  assert.equal(stored.records[0].observedSignature, "target-buy-signature-2");
  assert.equal(stored.records[0].sourceWalletAddress, "RetrySourceWallet11111111111111111111111111");
  assert.equal(stored.records[0].errorText, null);
  assert.equal(stored.records[0].claimedAt, "2026-05-24T12:05:00.000Z");
});

test("JSON copy buy idempotency retry toggle still blocks claimed and submitted buys", async (t) => {
  const path = await tempStorePath(t);
  const store = createJsonCopyTradeBuyIdempotencyStore({ path });

  assert.equal((await store.claimBuy(claimInput())).claimed, true);
  const claimedDuplicate = await store.claimBuy(claimInput({
    observedSignature: "target-buy-signature-2",
    retryFailed: true
  }));

  assert.equal(claimedDuplicate.claimed, false);
  assert.equal(claimedDuplicate.existing?.status, "claimed");

  await store.completeBuy(claimInput().key, {
    ok: true,
    status: 200,
    signature: "copy-buy-signature-1",
    errorText: null,
    raw: { signature: "copy-buy-signature-1" }
  });

  const submittedDuplicate = await store.claimBuy(claimInput({
    observedSignature: "target-buy-signature-3",
    retryFailed: true
  }));

  assert.equal(submittedDuplicate.claimed, false);
  assert.equal(submittedDuplicate.existing?.status, "submitted");
  assert.equal(submittedDuplicate.existing?.resultSignature, "copy-buy-signature-1");
});

test("Supabase copy buy idempotency retries only failed semantic records when enabled", async () => {
  const failedInput = claimInput();
  const fake = createFakeSupabaseIdempotencyClient([
    snakeCaseClaimRow(failedInput, "failed")
  ]);
  const store = createSupabaseCopyTradeBuyIdempotencyStore({ client: fake.client });
  const retry = await store.claimBuy(claimInput({
    observedSignature: "target-buy-signature-2",
    sourceWalletAddress: "RetrySourceWallet11111111111111111111111111",
    retryFailed: true,
    now: "2026-05-24T12:05:00.000Z"
  }));

  assert.equal(retry.claimed, true);
  assert.equal(retry.existing, null);
  assert.equal(fake.rows.length, 1);
  assert.equal(fake.rows[0].status, "claimed");
  assert.equal(fake.rows[0].idempotency_key, failedInput.key);
  assert.equal(fake.rows[0].observed_signature, "target-buy-signature-2");
  assert.equal(fake.rows[0].source_wallet_address, "RetrySourceWallet11111111111111111111111111");
  assert.equal(fake.rows[0].error_text, null);
  assert.equal(fake.rows[0].claimed_at, "2026-05-24T12:05:00.000Z");

  const claimedFake = createFakeSupabaseIdempotencyClient([
    snakeCaseClaimRow(claimInput(), "claimed")
  ]);
  const claimedStore = createSupabaseCopyTradeBuyIdempotencyStore({ client: claimedFake.client });
  const claimedDuplicate = await claimedStore.claimBuy(claimInput({
    observedSignature: "target-buy-signature-3",
    retryFailed: true
  }));

  assert.equal(claimedDuplicate.claimed, false);
  assert.equal(claimedDuplicate.existing?.status, "claimed");
  assert.equal(claimedFake.rows.length, 1);
  assert.equal(claimedFake.rows[0].observed_signature, "target-buy-signature-1");

  const submittedFake = createFakeSupabaseIdempotencyClient([
    {
      ...snakeCaseClaimRow(claimInput(), "submitted"),
      result_signature: "copy-buy-signature-1"
    }
  ]);
  const submittedStore = createSupabaseCopyTradeBuyIdempotencyStore({ client: submittedFake.client });
  const submittedDuplicate = await submittedStore.claimBuy(claimInput({
    observedSignature: "target-buy-signature-3",
    retryFailed: true
  }));

  assert.equal(submittedDuplicate.claimed, false);
  assert.equal(submittedDuplicate.existing?.status, "submitted");
  assert.equal(submittedDuplicate.existing?.resultSignature, "copy-buy-signature-1");
});

test("Supabase copy buy idempotency blocks failed semantic records by default", async () => {
  const failedInput = claimInput();
  const fake = createFakeSupabaseIdempotencyClient([
    snakeCaseClaimRow(failedInput, "failed")
  ]);
  const store = createSupabaseCopyTradeBuyIdempotencyStore({ client: fake.client });
  const duplicate = await store.claimBuy(claimInput({
    observedSignature: "target-buy-signature-2"
  }));

  assert.equal(duplicate.claimed, false);
  assert.equal(duplicate.existing?.status, "failed");
  assert.equal(fake.rows.length, 1);
  assert.equal(fake.rows[0].status, "failed");
  assert.equal(fake.rows[0].observed_signature, "target-buy-signature-1");
});

test("Supabase copy buy idempotency delegates to JSON fallback when table is unavailable", async (t) => {
  const path = await tempStorePath(t);
  const fallback = createJsonCopyTradeBuyIdempotencyStore({ path });
  const store = createSupabaseCopyTradeBuyIdempotencyStore({
    client: createMissingSupabaseIdempotencyClient(),
    fallback
  });

  assert.equal((await store.claimBuy(claimInput())).claimed, true);
  await store.failBuy(claimInput().key, "schema fallback failure");

  const retry = await store.claimBuy(claimInput({
    observedSignature: "target-buy-signature-2",
    retryFailed: true
  }));
  const stored = JSON.parse(await readFile(path, "utf8"));

  assert.equal(retry.claimed, true);
  assert.equal(stored.records.length, 1);
  assert.equal(stored.records[0].observedSignature, "target-buy-signature-2");
  assert.equal(stored.records[0].status, "claimed");
});

test("Supabase copy buy idempotency delegates to JSON fallback when provider check is outdated", async (t) => {
  const path = await tempStorePath(t);
  const fallback = createJsonCopyTradeBuyIdempotencyStore({ path });
  const store = createSupabaseCopyTradeBuyIdempotencyStore({
    client: createProviderConstraintSupabaseIdempotencyClient(),
    fallback
  });
  const input = claimInput({ provider: "geyser" });

  assert.equal((await store.claimBuy(input)).claimed, true);
  await store.completeBuy(input.key, {
    ok: true,
    status: 200,
    signature: "copy-buy-signature-geyser",
    errorText: null,
    raw: { signature: "copy-buy-signature-geyser" }
  });

  const stored = JSON.parse(await readFile(path, "utf8"));
  assert.equal(stored.records.length, 1);
  assert.equal(stored.records[0].provider, "geyser");
  assert.equal(stored.records[0].status, "submitted");
  assert.equal(stored.records[0].resultSignature, "copy-buy-signature-geyser");
});

test("JSON copy buy idempotency blocks same chat and mint across observed signatures", async (t) => {
  const path = await tempStorePath(t);
  const store = createJsonCopyTradeBuyIdempotencyStore({ path });

  const first = await store.claimBuy(claimInput());
  const second = await store.claimBuy(claimInput({
    sourceWalletAddress: "AnotherSourceWallet1111111111111111111111111",
    tradingWalletPublicKey: "AnotherTradingWallet11111111111111111111111",
    observedSignature: "target-buy-signature-2"
  }));

  assert.equal(first.claimed, true);
  assert.equal(second.claimed, false);
  assert.equal(second.existing?.observedSignature, "target-buy-signature-1");
});

test("JSON copy buy idempotency allows different chats and different mints", async (t) => {
  const path = await tempStorePath(t);
  const store = createJsonCopyTradeBuyIdempotencyStore({ path });

  const first = await store.claimBuy(claimInput());
  const differentMint = await store.claimBuy(claimInput({
    mint: "OtherMint11111111111111111111111111111111111",
    request: {
      ...request,
      mint: "OtherMint11111111111111111111111111111111111"
    },
    observedSignature: "target-buy-signature-2"
  }));
  const differentChat = await store.claimBuy(claimInput({
    chatId: "chat-2",
    observedSignature: "target-buy-signature-3"
  }));

  assert.equal(first.claimed, true);
  assert.equal(differentMint.claimed, true);
  assert.equal(differentChat.claimed, true);
});

test("JSON copy buy idempotency serializes concurrent claims", async (t) => {
  const path = await tempStorePath(t);
  const store = createJsonCopyTradeBuyIdempotencyStore({ path });

  const attempts = await Promise.all(Array.from({ length: 12 }, () => store.claimBuy(claimInput())));
  const claimed = attempts.filter((attempt) => attempt.claimed);
  const duplicates = attempts.filter((attempt) => !attempt.claimed);

  assert.equal(claimed.length, 1);
  assert.equal(duplicates.length, 11);
  assert.equal(duplicates.every((attempt) => attempt.existing?.status === "claimed"), true);
});
