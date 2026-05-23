create table if not exists public.telegram_subscribers (
  chat_id text primary key,
  mode text null check (mode in ('migrations', 'newtokens', 'both')),
  copy_wallet_address text null,
  copy_wallet_addresses text[] not null default '{}',
  copy_amount_sol numeric null check (copy_amount_sol is null or copy_amount_sol > 0),
  copy_target_wallet_address text null,
  verified_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create table if not exists public.telegram_watched_wallets (
  chat_id text not null references public.telegram_subscribers(chat_id) on delete cascade,
  address text not null,
  label text null,
  added_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key (chat_id, address)
);

create index if not exists telegram_watched_wallets_address_idx
  on public.telegram_watched_wallets(address);

create index if not exists telegram_watched_wallets_chat_id_idx
  on public.telegram_watched_wallets(chat_id);

alter table public.telegram_subscribers enable row level security;
alter table public.telegram_watched_wallets enable row level security;
