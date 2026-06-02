drop index if exists public.telegram_copytrade_buy_idempotency_semantic_buy_idx;

create index if not exists telegram_copytrade_buy_idempotency_semantic_lookup_idx
  on public.telegram_copytrade_buy_idempotency(chat_id, mint, action, claimed_at desc);

create unique index if not exists telegram_copytrade_buy_idempotency_observed_buy_idx
  on public.telegram_copytrade_buy_idempotency(
    chat_id,
    source_wallet_address,
    observed_signature,
    action
  );
