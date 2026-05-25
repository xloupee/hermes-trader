delete from public.telegram_copytrade_buy_idempotency existing
using (
  select
    ctid,
    row_number() over (
      partition by chat_id, mint, action
      order by claimed_at asc, updated_at asc, idempotency_key asc
    ) as duplicate_rank
  from public.telegram_copytrade_buy_idempotency
) ranked
where existing.ctid = ranked.ctid
  and ranked.duplicate_rank > 1;

drop index if exists public.telegram_copytrade_buy_idempotency_semantic_buy_idx;

create unique index if not exists telegram_copytrade_buy_idempotency_semantic_buy_idx
  on public.telegram_copytrade_buy_idempotency(chat_id, mint, action);
