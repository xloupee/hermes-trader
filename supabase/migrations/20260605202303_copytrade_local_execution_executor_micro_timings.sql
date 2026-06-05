alter table public.copytrade_local_executions
  add column if not exists executor_queue_us bigint,
  add column if not exists guards_us bigint,
  add column if not exists unsigned_build_us bigint,
  add column if not exists sign_us bigint,
  add column if not exists serialize_us bigint;

create index if not exists copytrade_local_executions_executor_micro_timings_idx
  on public.copytrade_local_executions (executor_queue_us, unsigned_build_us, sign_us)
  where executor_queue_us is not null
     or unsigned_build_us is not null
     or sign_us is not null;

notify pgrst, 'reload schema';
