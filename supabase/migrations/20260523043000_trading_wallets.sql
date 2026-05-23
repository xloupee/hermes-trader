create table if not exists public.telegram_trading_wallets (
  chat_id text primary key references public.telegram_subscribers(chat_id) on delete cascade,
  public_key text not null,
  encrypted_api_key text not null,
  api_key_last4 text not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create index if not exists telegram_trading_wallets_public_key_idx
  on public.telegram_trading_wallets(public_key);

alter table public.telegram_trading_wallets enable row level security;
