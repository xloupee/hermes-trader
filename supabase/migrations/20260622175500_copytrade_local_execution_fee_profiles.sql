alter table public.copytrade_local_executions
  add column if not exists fee_profile_name text,
  add column if not exists selected_priority_fee_micro_lamports bigint,
  add column if not exists selected_helius_tip_lamports bigint,
  add column if not exists source_position_bucket text,
  add column if not exists fee_reason text,
  add column if not exists fee_cap_hit boolean not null default false;

create index if not exists copytrade_local_executions_fee_profile_idx
  on public.copytrade_local_executions (fee_profile_name, source_position_bucket, created_at desc);

notify pgrst, 'reload schema';
