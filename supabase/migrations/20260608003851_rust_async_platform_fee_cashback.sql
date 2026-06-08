alter table public.telegram_cashback_ledger
  add column if not exists platform_fee_bps integer null,
  add column if not exists platform_fee_treasury text null,
  add column if not exists platform_fee_collection_status text not null default 'not_required',
  add column if not exists platform_fee_transfer_signature text null,
  add column if not exists platform_fee_collection_error text null,
  add column if not exists platform_fee_collection_attempts integer not null default 0,
  add column if not exists platform_fee_collection_updated_at timestamptz null;

alter table public.telegram_cashback_ledger
  drop constraint if exists telegram_cashback_ledger_platform_fee_bps_check,
  drop constraint if exists telegram_cashback_ledger_platform_fee_collection_status_check,
  drop constraint if exists telegram_cashback_ledger_platform_fee_collection_attempts_check;

alter table public.telegram_cashback_ledger
  add constraint telegram_cashback_ledger_platform_fee_bps_check
  check (platform_fee_bps is null or (platform_fee_bps >= 0 and platform_fee_bps <= 10000)),
  add constraint telegram_cashback_ledger_platform_fee_collection_status_check
  check (platform_fee_collection_status in ('not_required', 'pending', 'submitted', 'confirmed', 'failed')),
  add constraint telegram_cashback_ledger_platform_fee_collection_attempts_check
  check (platform_fee_collection_attempts >= 0);

create index if not exists telegram_cashback_ledger_platform_fee_collection_idx
  on public.telegram_cashback_ledger(platform_fee_collection_status, platform_fee_collection_updated_at)
  where platform_fee_collection_status in ('pending', 'submitted');

create index if not exists telegram_cashback_ledger_platform_fee_transfer_signature_idx
  on public.telegram_cashback_ledger(platform_fee_transfer_signature)
  where platform_fee_transfer_signature is not null;

comment on column public.telegram_cashback_ledger.platform_fee_bps is
  'Platform fee basis points used when this cashback ledger row was created.';
comment on column public.telegram_cashback_ledger.platform_fee_treasury is
  'Treasury wallet that should receive or received the collected platform fee.';
comment on column public.telegram_cashback_ledger.platform_fee_collection_status is
  'Async platform-fee collection lifecycle for rows whose cashback depends on a separate post-buy fee transfer.';
comment on column public.telegram_cashback_ledger.platform_fee_transfer_signature is
  'Separate Solana transaction signature used to collect an async Rust-buy platform fee.';

notify pgrst, 'reload schema';
