create table if not exists public.telegram_copytrade_wallets (
  chat_id text not null references public.telegram_subscribers(chat_id) on delete cascade,
  address text not null,
  label text null,
  added_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key (chat_id, address)
);

insert into public.telegram_copytrade_wallets (chat_id, address, label, added_at, updated_at)
select
  subscriber.chat_id,
  subscriber.copy_target_wallet_address,
  watched.label,
  coalesce(watched.added_at, subscriber.updated_at),
  greatest(coalesce(watched.updated_at, subscriber.updated_at), subscriber.updated_at)
from public.telegram_subscribers subscriber
left join public.telegram_watched_wallets watched
  on watched.chat_id = subscriber.chat_id
  and watched.address = subscriber.copy_target_wallet_address
where subscriber.copy_target_wallet_address is not null
on conflict (chat_id, address) do update
set label = coalesce(public.telegram_copytrade_wallets.label, excluded.label),
    updated_at = greatest(public.telegram_copytrade_wallets.updated_at, excluded.updated_at);

delete from public.telegram_watched_wallets watched
using public.telegram_subscribers subscriber
where subscriber.chat_id = watched.chat_id
  and subscriber.copy_target_wallet_address = watched.address;

create index if not exists telegram_copytrade_wallets_address_idx
  on public.telegram_copytrade_wallets(address);

create index if not exists telegram_copytrade_wallets_chat_id_idx
  on public.telegram_copytrade_wallets(chat_id);

alter table public.telegram_copytrade_wallets enable row level security;
