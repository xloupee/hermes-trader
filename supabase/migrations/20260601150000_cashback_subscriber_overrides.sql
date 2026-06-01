alter table public.telegram_subscribers
  add column if not exists cashback_enabled_override boolean null,
  add column if not exists cashback_fee_share_bps_override integer null,
  add column if not exists cashback_override_note text null,
  add column if not exists cashback_override_updated_by text null,
  add column if not exists cashback_override_updated_at timestamptz null;

alter table public.telegram_subscribers
  drop constraint if exists telegram_subscribers_cashback_fee_share_bps_override_check;

alter table public.telegram_subscribers
  add constraint telegram_subscribers_cashback_fee_share_bps_override_check
  check (
    cashback_fee_share_bps_override is null or
    (cashback_fee_share_bps_override >= 0 and cashback_fee_share_bps_override <= 10000)
  );

alter table public.telegram_cashback_ledger
  add column if not exists cashback_fee_share_bps integer null,
  add column if not exists entry_type text not null default 'trade',
  add column if not exists adjustment_reason text null,
  add column if not exists adjusted_by text null;

alter table public.telegram_cashback_ledger
  drop constraint if exists telegram_cashback_ledger_cashback_fee_share_bps_check,
  drop constraint if exists telegram_cashback_ledger_entry_type_check,
  drop constraint if exists telegram_cashback_ledger_action_check,
  drop constraint if exists telegram_cashback_ledger_cashback_lamports_check;

alter table public.telegram_cashback_ledger
  add constraint telegram_cashback_ledger_cashback_fee_share_bps_check
  check (cashback_fee_share_bps is null or (cashback_fee_share_bps >= 0 and cashback_fee_share_bps <= 10000)),
  add constraint telegram_cashback_ledger_entry_type_check
  check (entry_type in ('trade', 'manual_adjustment')),
  add constraint telegram_cashback_ledger_action_check
  check (action in ('buy', 'sell', 'adjustment'));

create index if not exists telegram_cashback_ledger_adjustments_idx
  on public.telegram_cashback_ledger(chat_id, trading_wallet_public_key, created_at desc)
  where entry_type = 'manual_adjustment';

comment on column public.telegram_subscribers.cashback_enabled_override is
  'Subscriber-level cashback enabled override. Null means use CASHBACK_ENABLED.';
comment on column public.telegram_subscribers.cashback_fee_share_bps_override is
  'Subscriber-level cashback fee-share override in basis points. Null means use CASHBACK_FEE_SHARE_BPS.';
comment on column public.telegram_subscribers.cashback_override_note is
  'Operator note explaining the current cashback subscriber override.';
comment on column public.telegram_cashback_ledger.cashback_fee_share_bps is
  'Cashback fee-share basis points resolved at accrual time for trade rows.';
comment on column public.telegram_cashback_ledger.entry_type is
  'Ledger row type: trade accrual or audited manual adjustment.';
comment on column public.telegram_cashback_ledger.adjustment_reason is
  'Operator-supplied reason for manual adjustment rows.';
comment on column public.telegram_cashback_ledger.adjusted_by is
  'Operator metadata for manual adjustment rows.';

notify pgrst, 'reload schema';
