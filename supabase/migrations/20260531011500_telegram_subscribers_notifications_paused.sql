alter table public.telegram_subscribers
  add column if not exists notifications_paused boolean not null default false;

comment on column public.telegram_subscribers.notifications_paused is
  'When true, the chat remains verified and retains settings, but token alerts, wallet alerts, and copytrade execution are paused.';

notify pgrst, 'reload schema';
