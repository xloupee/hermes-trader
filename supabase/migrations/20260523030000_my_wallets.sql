create table if not exists public.telegram_my_wallets (
  chat_id text not null references public.telegram_subscribers(chat_id) on delete cascade,
  address text not null,
  label text null,
  added_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key (chat_id, address)
);

update public.telegram_subscribers
set copy_wallet_address = null,
    copy_wallet_addresses = '{}'
where copy_wallet_address is not null
   or coalesce(array_length(copy_wallet_addresses, 1), 0) > 0;

create index if not exists telegram_my_wallets_address_idx
  on public.telegram_my_wallets(address);

create index if not exists telegram_my_wallets_chat_id_idx
  on public.telegram_my_wallets(chat_id);

alter table public.telegram_my_wallets enable row level security;
