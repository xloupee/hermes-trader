alter table public.copytrade_local_executions
  add column if not exists signed_tx_bytes integer,
  add column if not exists writable_account_count integer,
  add column if not exists compute_unit_limit integer,
  add column if not exists selected_tip_account text,
  add column if not exists source_compute_unit_limit integer,
  add column if not exists source_compute_unit_price_micro_lamports bigint,
  add column if not exists compute_units_consumed bigint,
  add column if not exists cost_units bigint,
  add column if not exists transaction_meta_error text,
  add column if not exists blockhash text,
  add column if not exists blockhash_source_rpc text,
  add column if not exists blockhash_commitment text,
  add column if not exists blockhash_context_slot bigint,
  add column if not exists blockhash_age_ms bigint,
  add column if not exists blockhash_selection_strategy text,
  add column if not exists account_priority_fee_enabled boolean not null default false,
  add column if not exists account_priority_fee_micro_lamports bigint,
  add column if not exists account_priority_fee_age_ms bigint,
  add column if not exists account_priority_fee_sample_count integer,
  add column if not exists account_priority_fee_account_count integer,
  add column if not exists account_priority_fee_applied boolean not null default false,
  add column if not exists account_priority_fee_reason text;

create index if not exists copytrade_local_executions_landing_shape_idx
  on public.copytrade_local_executions (route_layout, instruction_count, signed_tx_bytes, writable_account_count, created_at desc);

create index if not exists copytrade_local_executions_selected_tip_account_idx
  on public.copytrade_local_executions (selected_tip_account, created_at desc)
  where selected_tip_account is not null;

create index if not exists copytrade_local_executions_blockhash_commitment_idx
  on public.copytrade_local_executions (blockhash_commitment, blockhash_selection_strategy, created_at desc)
  where blockhash_commitment is not null;

create index if not exists copytrade_local_executions_account_priority_fee_idx
  on public.copytrade_local_executions (account_priority_fee_applied, account_priority_fee_micro_lamports, created_at desc)
  where account_priority_fee_enabled is true;

notify pgrst, 'reload schema';
