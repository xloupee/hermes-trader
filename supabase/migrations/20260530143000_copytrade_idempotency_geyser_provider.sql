alter table public.telegram_copytrade_buy_idempotency
  drop constraint if exists telegram_copytrade_buy_idempotency_provider_check;

alter table public.telegram_copytrade_buy_idempotency
  add constraint telegram_copytrade_buy_idempotency_provider_check
  check (provider in ('helius', 'pumpportal', 'geyser', 'yellowstone'));
