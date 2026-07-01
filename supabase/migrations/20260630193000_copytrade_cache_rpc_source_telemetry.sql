alter table public.copytrade_local_executions
  add column if not exists account_priority_fee_source_rpc text,
  add column if not exists copy_wallet_balance_lamports bigint,
  add column if not exists copy_wallet_balance_required_lamports bigint,
  add column if not exists copy_wallet_balance_fetched_at_ms numeric,
  add column if not exists copy_wallet_balance_age_ms numeric,
  add column if not exists copy_wallet_balance_source_rpc text,
  add column if not exists copy_wallet_balance_reason text;

create index if not exists copytrade_local_executions_cache_rpc_source_idx
  on public.copytrade_local_executions (
    account_priority_fee_source_rpc,
    copy_wallet_balance_source_rpc,
    created_at desc
  );
