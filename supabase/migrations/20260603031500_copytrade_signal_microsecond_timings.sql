alter table public.copytrade_signal_observations
  add column if not exists deserialize_us bigint,
  add column if not exists parse_us bigint,
  add column if not exists local_detect_us bigint;

create index if not exists copytrade_signal_observations_local_detect_us_idx
  on public.copytrade_signal_observations (local_detect_us)
  where local_detect_us is not null;

notify pgrst, 'reload schema';
