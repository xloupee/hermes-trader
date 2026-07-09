alter table public.telegram_cashback_ledger
  add column if not exists platform_fee_lease_token text null,
  add column if not exists platform_fee_lease_expires_at timestamptz null,
  add column if not exists platform_fee_transaction_base64 text null,
  add column if not exists platform_fee_recent_blockhash text null,
  add column if not exists platform_fee_last_valid_block_height bigint null;

alter table public.telegram_cashback_ledger
  drop constraint if exists telegram_cashback_ledger_platform_fee_signed_payload_check,
  drop constraint if exists telegram_cashback_ledger_platform_fee_lease_check;

alter table public.telegram_cashback_ledger
  add constraint telegram_cashback_ledger_platform_fee_signed_payload_check
  check (
    platform_fee_transaction_base64 is null or
    platform_fee_transfer_signature is not null
  ),
  add constraint telegram_cashback_ledger_platform_fee_lease_check
  check (
    (platform_fee_lease_token is null and platform_fee_lease_expires_at is null) or
    (platform_fee_lease_token is not null and platform_fee_lease_expires_at is not null)
  );

drop index if exists public.telegram_cashback_ledger_platform_fee_collection_idx;
create index telegram_cashback_ledger_platform_fee_collection_idx
  on public.telegram_cashback_ledger(
    platform_fee_collection_status,
    platform_fee_lease_expires_at,
    platform_fee_collection_updated_at
  )
  where platform_fee_collection_status in ('pending', 'submitted');

comment on column public.telegram_cashback_ledger.platform_fee_lease_token is
  'Opaque worker token that owns the current async platform-fee collection lease.';
comment on column public.telegram_cashback_ledger.platform_fee_lease_expires_at is
  'Lease expiry allowing another service-role collector to recover abandoned work.';
comment on column public.telegram_cashback_ledger.platform_fee_transaction_base64 is
  'Signed Solana transaction bytes persisted before broadcast and reused verbatim on retry.';
comment on column public.telegram_cashback_ledger.platform_fee_recent_blockhash is
  'Recent blockhash embedded in the persisted signed platform-fee transaction.';
comment on column public.telegram_cashback_ledger.platform_fee_last_valid_block_height is
  'Last valid block height returned with the persisted transaction recent blockhash.';

-- This table is an internal service-role queue. Preserve RLS defense in depth and
-- do not expose signed transaction bytes to anon or authenticated clients.
alter table public.telegram_cashback_ledger enable row level security;
revoke all on table public.telegram_cashback_ledger from anon, authenticated;
grant select, insert, update, delete on table public.telegram_cashback_ledger to service_role;

notify pgrst, 'reload schema';
