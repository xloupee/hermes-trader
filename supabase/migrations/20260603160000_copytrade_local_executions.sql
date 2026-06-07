create table if not exists public.copytrade_local_executions (
  id bigserial primary key,
  created_at timestamptz not null default now(),
  observed_at_ms bigint not null,
  execution_at_ms bigint,
  provider text not null,
  source text not null,
  endpoint text,
  observed_wallet text not null,
  copy_wallet text,
  observed_signature text not null,
  send_signature text,
  slot bigint not null,
  copy_slot bigint,
  slot_delta_from_observed integer,
  target_slot bigint,
  target_tx_index integer,
  copy_tx_index integer,
  same_slot_tx_delta integer,
  position_unavailable_reason text,
  selected_route text not null,
  route_layout text,
  mint text not null,
  observed_action text not null,
  observed_sol_amount double precision,
  max_copy_sol double precision,
  decision text not null,
  reason text,
  signed boolean not null default false,
  simulated boolean not null default false,
  sent boolean not null default false,
  dry_run boolean not null default true,
  send_enabled boolean not null default false,
  send_rpc_winner text,
  send_rpc_url_count integer,
  send_rpc_errors jsonb not null default '[]'::jsonb,
  simulation_requested boolean not null default false,
  instruction_count integer not null default 0,
  simulation_units_consumed bigint,
  fill_token_delta double precision,
  copy_wallet_sol_delta double precision,
  gross_copy_spend_sol double precision,
  network_fee_sol double precision,
  extra_spend_beyond_observed_sol double precision,
  extra_spend_beyond_observed_and_network_fee_sol double precision,
  observed_to_signed_ms integer,
  observed_to_simulation_completed_ms integer,
  observed_to_send_submitted_ms integer,
  observed_to_signature_returned_ms integer,
  auto_sell_enabled boolean not null default false,
  auto_sell_delay_ms integer,
  auto_sell_attempted boolean not null default false,
  auto_sell_signed boolean not null default false,
  auto_sell_simulated boolean not null default false,
  auto_sell_sent boolean not null default false,
  auto_sell_decision text,
  auto_sell_reason text,
  auto_sell_token_amount_raw bigint,
  auto_sell_send_signature text,
  auto_sell_send_rpc_winner text,
  auto_sell_send_rpc_url_count integer,
  auto_sell_send_rpc_errors jsonb not null default '[]'::jsonb,
  buy_signature_to_auto_sell_submitted_ms integer,
  buy_signature_to_auto_sell_signature_returned_ms integer,
  raw_execution jsonb not null default '{}'::jsonb,
  chain_report jsonb not null default '{}'::jsonb
);

alter table public.copytrade_local_executions
  add column if not exists auto_sell_enabled boolean not null default false,
  add column if not exists auto_sell_delay_ms integer,
  add column if not exists auto_sell_attempted boolean not null default false,
  add column if not exists auto_sell_signed boolean not null default false,
  add column if not exists auto_sell_simulated boolean not null default false,
  add column if not exists auto_sell_sent boolean not null default false,
  add column if not exists auto_sell_decision text,
  add column if not exists auto_sell_reason text,
  add column if not exists auto_sell_token_amount_raw bigint,
  add column if not exists auto_sell_send_signature text,
  add column if not exists buy_signature_to_auto_sell_submitted_ms integer,
  add column if not exists buy_signature_to_auto_sell_signature_returned_ms integer,
  add column if not exists target_slot bigint,
  add column if not exists target_tx_index integer,
  add column if not exists copy_tx_index integer,
  add column if not exists same_slot_tx_delta integer,
  add column if not exists position_unavailable_reason text,
  add column if not exists send_rpc_winner text,
  add column if not exists send_rpc_url_count integer,
  add column if not exists send_rpc_errors jsonb not null default '[]'::jsonb,
  add column if not exists auto_sell_send_rpc_winner text,
  add column if not exists auto_sell_send_rpc_url_count integer,
  add column if not exists auto_sell_send_rpc_errors jsonb not null default '[]'::jsonb;

create index if not exists copytrade_local_executions_created_at_idx
  on public.copytrade_local_executions (created_at desc);

create index if not exists copytrade_local_executions_observed_signature_idx
  on public.copytrade_local_executions (observed_signature);

create index if not exists copytrade_local_executions_wallet_created_idx
  on public.copytrade_local_executions (observed_wallet, created_at desc);

create index if not exists copytrade_local_executions_mint_created_idx
  on public.copytrade_local_executions (mint, created_at desc);

create index if not exists copytrade_local_executions_decision_created_idx
  on public.copytrade_local_executions (decision, created_at desc);

create index if not exists copytrade_local_executions_auto_sell_created_idx
  on public.copytrade_local_executions (auto_sell_decision, created_at desc);

create index if not exists copytrade_local_executions_position_created_idx
  on public.copytrade_local_executions (slot_delta_from_observed, same_slot_tx_delta, created_at desc);

create unique index if not exists copytrade_local_executions_unique_idx
  on public.copytrade_local_executions (
    provider,
    observed_signature,
    observed_wallet,
    observed_action,
    mint
  );

alter table public.copytrade_local_executions enable row level security;

grant select, insert, update on public.copytrade_local_executions to service_role;
grant usage, select on sequence public.copytrade_local_executions_id_seq to service_role;

notify pgrst, 'reload schema';
