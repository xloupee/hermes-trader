# Per-Subscriber Cashback Overrides

Cashback defaults still come from the global env config:

- `CASHBACK_ENABLED`
- `CASHBACK_FEE_SHARE_BPS` (defaults to `4000`, i.e. 40% of collected platform fees)
- `CASHBACK_MIN_CLAIM_SOL`
- `CASHBACK_MAX_PAYOUT_SOL_PER_DAY`

Operators can override enablement or fee-share basis points for one Telegram subscriber without changing global defaults.
The base cashback rate for subscribers without an override is 40%.

## Inspect Effective Config

```bash
npm run cashback-admin -- show <chat_id>
```

The output shows the effective value and whether it came from `global` config or a `subscriber_override`.

## Set Overrides

```bash
npm run cashback-admin -- set-enabled <chat_id> true --updated-by <operator> --note "VIP cashback"
npm run cashback-admin -- set-enabled <chat_id> false --updated-by <operator> --note "disabled by request"
npm run cashback-admin -- set-fee-share <chat_id> 5000 --updated-by <operator> --note "50 percent fee share"
```

Fee-share values are basis points from `0` to `10000`.

## Clear Overrides

```bash
npm run cashback-admin -- clear-enabled <chat_id> --updated-by <operator> --note "back to default"
npm run cashback-admin -- clear-fee-share <chat_id> --updated-by <operator> --note "back to default"
npm run cashback-admin -- clear-all <chat_id> --updated-by <operator> --note "back to defaults"
```

Clearing an override restores fallback to the global env value.

## Manual Adjustments

Manual corrections and bonuses are explicit ledger rows. They do not rewrite balances.

```bash
npm run cashback-admin -- adjust <chat_id> <trading_wallet_public_key> 1000000 --reason "bonus" --updated-by <operator>
npm run cashback-admin -- adjust <chat_id> <trading_wallet_public_key> -1000000 --reason "correction" --updated-by <operator>
```

Adjustment amounts are lamports and may be positive or negative. Every adjustment requires a reason and operator metadata.

## Accrual Timing

Overrides are resolved when a new cashback ledger row is created. Existing ledger rows keep their original `cashback_lamports` and `cashback_fee_share_bps`; changing an override does not recalculate history.
