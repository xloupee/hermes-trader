alter table public.telegram_subscribers
  add column if not exists cashback_payout_wallet_address text null;

comment on column public.telegram_subscribers.cashback_payout_wallet_address is
  'User-selected wallet address that receives claimed cashback payouts.';

notify pgrst, 'reload schema';
