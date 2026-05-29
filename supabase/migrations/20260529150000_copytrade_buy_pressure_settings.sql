alter table public.telegram_subscribers
  add column if not exists copy_trade_buy_pressure_sell_enabled boolean not null default false,
  add column if not exists copy_trade_buy_pressure_sell_timeout_ms integer null
    check (copy_trade_buy_pressure_sell_timeout_ms is null or copy_trade_buy_pressure_sell_timeout_ms > 0);
