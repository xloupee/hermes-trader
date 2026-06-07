create table if not exists public.copytrade_signal_observations (
  id bigserial primary key,
  created_at timestamptz not null default now(),
  provider text not null,
  source text not null,
  endpoint text,
  target_wallet text not null,
  signature text not null,
  slot bigint not null,
  action text not null,
  mint text not null,
  route text not null,
  observed_at_ms bigint not null,
  block_time_ms bigint,
  observed_minus_block_time_ms bigint,
  sol_amount double precision,
  token_amount double precision,
  copyable boolean not null default false,
  raw_event jsonb not null default '{}'::jsonb,
  unique (provider, signature, target_wallet, action, mint)
);

create index if not exists copytrade_signal_observations_created_at_idx
  on public.copytrade_signal_observations (created_at desc);

create index if not exists copytrade_signal_observations_target_created_idx
  on public.copytrade_signal_observations (target_wallet, created_at desc);

create index if not exists copytrade_signal_observations_mint_created_idx
  on public.copytrade_signal_observations (mint, created_at desc);

create index if not exists copytrade_signal_observations_signature_idx
  on public.copytrade_signal_observations (signature);

alter table public.copytrade_signal_observations enable row level security;

grant select, insert on public.copytrade_signal_observations to service_role;
grant usage, select on sequence public.copytrade_signal_observations_id_seq to service_role;

notify pgrst, 'reload schema';
