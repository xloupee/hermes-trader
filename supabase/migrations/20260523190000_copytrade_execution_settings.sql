alter table public.telegram_subscribers
  add column if not exists copy_trade_buy_slippage_percent numeric null
    check (copy_trade_buy_slippage_percent is null or (copy_trade_buy_slippage_percent >= 0.1 and copy_trade_buy_slippage_percent <= 100)),
  add column if not exists copy_trade_buy_priority_fee_sol numeric null
    check (copy_trade_buy_priority_fee_sol is null or (copy_trade_buy_priority_fee_sol > 0 and copy_trade_buy_priority_fee_sol <= 1)),
  add column if not exists copy_trade_sell_slippage_percent numeric null
    check (copy_trade_sell_slippage_percent is null or (copy_trade_sell_slippage_percent >= 0.1 and copy_trade_sell_slippage_percent <= 100)),
  add column if not exists copy_trade_sell_priority_fee_sol numeric null
    check (copy_trade_sell_priority_fee_sol is null or (copy_trade_sell_priority_fee_sol > 0 and copy_trade_sell_priority_fee_sol <= 1));
