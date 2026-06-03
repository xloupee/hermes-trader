alter table public.copytrade_signal_observations
  add column if not exists grpc_message_received_at_ms bigint,
  add column if not exists entries_deserialized_at_ms bigint,
  add column if not exists trade_parsed_at_ms bigint,
  add column if not exists deserialize_ms bigint,
  add column if not exists parse_ms bigint,
  add column if not exists local_detect_ms bigint,
  add column if not exists grpc_received_minus_block_time_ms bigint;

create index if not exists copytrade_signal_observations_local_detect_idx
  on public.copytrade_signal_observations (local_detect_ms)
  where local_detect_ms is not null;

create index if not exists copytrade_signal_observations_grpc_blocktime_idx
  on public.copytrade_signal_observations (grpc_received_minus_block_time_ms)
  where grpc_received_minus_block_time_ms is not null;

notify pgrst, 'reload schema';
