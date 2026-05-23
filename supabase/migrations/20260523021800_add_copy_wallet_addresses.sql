alter table public.telegram_subscribers
  add column if not exists copy_wallet_addresses text[] not null default '{}';

update public.telegram_subscribers
set copy_wallet_addresses = array[copy_wallet_address]
where copy_wallet_address is not null
  and coalesce(array_length(copy_wallet_addresses, 1), 0) = 0;
