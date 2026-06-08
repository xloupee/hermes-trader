drop index if exists public.copytrade_local_executions_unique_idx;

delete from public.copytrade_local_executions kept
using public.copytrade_local_executions removed
where kept.ctid < removed.ctid
  and kept.provider = removed.provider
  and kept.observed_signature = removed.observed_signature
  and kept.observed_wallet = removed.observed_wallet
  and kept.copy_wallet is not distinct from removed.copy_wallet
  and kept.observed_action = removed.observed_action
  and kept.mint = removed.mint;

create unique index copytrade_local_executions_unique_idx
  on public.copytrade_local_executions (
    provider,
    observed_signature,
    observed_wallet,
    copy_wallet,
    observed_action,
    mint
  ) nulls not distinct;

notify pgrst, 'reload schema';
