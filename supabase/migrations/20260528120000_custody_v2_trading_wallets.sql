alter table if exists public.telegram_trading_wallets
  add column if not exists provider text null,
  add column if not exists kind text null,
  add column if not exists encrypted_secret_key text null,
  add column if not exists secret_key_format text null,
  add column if not exists key_last4 text null;

alter table if exists public.telegram_subscribers
  add column if not exists active_trading_wallet_public_key text null;

alter table if exists public.telegram_trading_wallets
  alter column encrypted_api_key drop not null;

update public.telegram_trading_wallets
set provider = coalesce(provider, 'pumpportal-lightning'),
    kind = coalesce(kind, 'pumpportal-lightning'),
    key_last4 = coalesce(key_last4, api_key_last4)
where provider is null
   or kind is null
   or key_last4 is null;

comment on column public.telegram_trading_wallets.provider is
  'Custody provider. Legacy rows default in application code to pumpportal-lightning; local signing rows use local-solana.';

comment on column public.telegram_trading_wallets.kind is
  'Wallet custody kind. Kept nullable for safe rollout compatibility with existing PumpPortal rows.';

comment on column public.telegram_trading_wallets.encrypted_secret_key is
  'Encrypted local Solana secret key for local-solana wallets. Never store plaintext key material here.';

comment on column public.telegram_trading_wallets.secret_key_format is
  'Encoding for encrypted local Solana secret key plaintext before encryption, currently base64.';

comment on column public.telegram_trading_wallets.key_last4 is
  'Non-secret suffix used only for Telegram display and operator checks.';

update public.telegram_subscribers subscriber
set active_trading_wallet_public_key = wallet.public_key
from public.telegram_trading_wallets wallet
where subscriber.chat_id = wallet.chat_id
  and subscriber.active_trading_wallet_public_key is null;

alter table if exists public.telegram_trading_wallets
  drop constraint if exists telegram_trading_wallets_pkey;

alter table if exists public.telegram_trading_wallets
  add constraint telegram_trading_wallets_pkey primary key (chat_id, public_key);

create index if not exists telegram_trading_wallets_chat_id_created_at_idx
  on public.telegram_trading_wallets(chat_id, created_at);

alter table if exists public.telegram_copytrade_executions
  drop constraint if exists telegram_copytrade_executions_status_check;

alter table if exists public.telegram_copytrade_executions
  add constraint telegram_copytrade_executions_status_check
  check (status in ('submitted', 'failed', 'skipped', 'simulated', 'confirmed', 'expired'));

comment on column public.telegram_subscribers.active_trading_wallet_public_key is
  'Active trading wallet public key for the chat. Pairs with telegram_trading_wallets(chat_id, public_key).';

comment on table public.telegram_trading_wallets is
  'Trading wallets for PumpPortal Lightning and local Solana signing custody. Primary key is (chat_id, public_key) so one chat can keep multiple wallets.';
