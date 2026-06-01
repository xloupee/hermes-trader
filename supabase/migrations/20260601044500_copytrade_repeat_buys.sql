alter table public.telegram_copytrade_buy_idempotency
  drop constraint if exists telegram_copytrade_buy_idempotency_provider_check;

alter table public.telegram_copytrade_buy_idempotency
  add constraint telegram_copytrade_buy_idempotency_provider_check
  check (provider in ('helius', 'pumpportal', 'geyser', 'yellowstone', 'shredstream'));

drop index if exists public.telegram_copytrade_buy_idempotency_semantic_buy_idx;

create index if not exists telegram_copytrade_buy_idempotency_semantic_buy_idx
  on public.telegram_copytrade_buy_idempotency(chat_id, mint, action);
