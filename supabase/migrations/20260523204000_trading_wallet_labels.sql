alter table public.telegram_trading_wallets
  add column if not exists label text null;
