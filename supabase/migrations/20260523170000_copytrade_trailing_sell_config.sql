alter table public.telegram_copytrade_wallets
  add column if not exists trailing_sell_config jsonb;
