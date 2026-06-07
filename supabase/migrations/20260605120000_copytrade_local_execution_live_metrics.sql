alter table public.copytrade_local_executions
  add column if not exists feed_received_at_ms bigint,
  add column if not exists decoded_at_ms bigint,
  add column if not exists matched_at_ms bigint,
  add column if not exists planned_at_ms bigint,
  add column if not exists built_at_ms bigint,
  add column if not exists feed_received_to_decoded_us bigint,
  add column if not exists decoded_to_matched_us bigint,
  add column if not exists matched_to_planned_ms integer,
  add column if not exists planned_to_built_ms integer,
  add column if not exists batch_transaction_count bigint,
  add column if not exists matched_transaction_index bigint,
  add column if not exists batch_scan_us bigint,
  add column if not exists tx_parse_us bigint,
  add column if not exists account_expand_us bigint,
  add column if not exists wallet_match_us bigint,
  add column if not exists route_parse_us bigint,
  add column if not exists send_lane_ms integer,
  add column if not exists slot_delta integer,
  add column if not exists tx_delta integer;

create index if not exists copytrade_local_executions_send_lane_ms_idx
  on public.copytrade_local_executions (send_lane_ms)
  where send_lane_ms is not null;

create index if not exists copytrade_local_executions_live_parse_us_idx
  on public.copytrade_local_executions (route_parse_us, wallet_match_us)
  where route_parse_us is not null;

notify pgrst, 'reload schema';
