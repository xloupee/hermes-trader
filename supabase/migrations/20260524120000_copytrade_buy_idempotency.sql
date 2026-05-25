create table if not exists public.telegram_copytrade_buy_idempotency (
  idempotency_key text primary key,
  chat_id text not null references public.telegram_subscribers(chat_id) on delete cascade,
  source_wallet_address text not null,
  trading_wallet_public_key text not null,
  observed_signature text not null,
  mint text not null,
  action text not null default 'buy' check (action = 'buy'),
  amount_sol numeric not null check (amount_sol > 0),
  provider text not null check (provider in ('helius', 'pumpportal')),
  request jsonb not null,
  status text not null check (status in ('claimed', 'submitted', 'failed')),
  result_signature text null,
  error_text text null,
  http_status integer null,
  response jsonb null,
  claimed_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  completed_at timestamptz null
);

create index if not exists telegram_copytrade_buy_idempotency_chat_claimed_idx
  on public.telegram_copytrade_buy_idempotency(chat_id, claimed_at desc);

create index if not exists telegram_copytrade_buy_idempotency_observed_signature_idx
  on public.telegram_copytrade_buy_idempotency(observed_signature);

create unique index if not exists telegram_copytrade_buy_idempotency_semantic_buy_idx
  on public.telegram_copytrade_buy_idempotency(
    chat_id,
    trading_wallet_public_key,
    source_wallet_address,
    observed_signature,
    mint,
    action
  );

alter table public.telegram_copytrade_buy_idempotency enable row level security;
