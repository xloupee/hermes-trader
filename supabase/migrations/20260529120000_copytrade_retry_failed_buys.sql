alter table public.telegram_subscribers
  add column if not exists copy_trade_retry_failed_buys boolean not null default false;
