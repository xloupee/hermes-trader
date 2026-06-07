alter table public.copytrade_signal_observations
  add column if not exists batch_transaction_count bigint,
  add column if not exists matched_transaction_index bigint,
  add column if not exists batch_scan_us bigint,
  add column if not exists tx_parse_us bigint,
  add column if not exists account_expand_us bigint,
  add column if not exists wallet_match_us bigint,
  add column if not exists route_parse_us bigint;

create index if not exists copytrade_signal_observations_tx_parse_us_idx
  on public.copytrade_signal_observations (tx_parse_us)
  where tx_parse_us is not null;

notify pgrst, 'reload schema';
