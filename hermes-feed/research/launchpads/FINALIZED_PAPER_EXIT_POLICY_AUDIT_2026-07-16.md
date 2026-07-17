# Finalized paper exit-policy audit

## Finding

`hermes-launchpad-paper` accepted `paper_take_profit_bps`,
`paper_stop_loss_bps`, and `paper_max_hold_seconds`. The preliminary observer
plan emitted those values, but every typed finalized plan discarded them.
Finalized output retained only an independently simulated immediate
full-position round trip.

The immediate round trip is valuable quote evidence but is not a complete exit
plan. It cannot show that a future price or time trigger fired. The active paper
objective requires both independently quoted exit behavior and the configured
exit policy to remain observable.

## Representation

The existing finalized-plan schema is preserved. A new additive `exit_plan`
object is emitted for Bow/LaunchHood V3, Clanker, Bankr/Doppler, current Pons,
and Hood whenever their typed quote passes finalization.

The object records:

- full-position take-profit, stop-loss, or maximum-hold strategy;
- the canonical WETH quote asset;
- configured take-profit and stop-loss basis points;
- independently derived WETH trigger thresholds based on the fixed paper entry
  size, with take profit rounded up and stop loss rounded down;
- explicit greater-than-or-equal take-profit, less-than-or-equal stop-loss, and
  elapsed-time trigger conditions;
- the maximum hold duration;
- a future independent warm full-position quote as the trigger authority;
- `not_evaluated_at_static_receipt_finalization`, preventing the immediate
  round-trip simulation from masquerading as a fired trigger; and
- `execution_eligible: false` and `broadcast: false`.

The existing `exit_expected_output`, `exit_min_receive`, and
`simulated_round_trip_return_bps` remain unchanged and explicitly describe the
same-reconciled-state immediate exit simulation.

## Fail-closed boundary

Finalization rejects a policy when take profit, stop loss, or maximum hold is
zero; stop loss is at least 100%; threshold arithmetic overflows; or entry-size
rounding collapses either threshold. This validation also covers finalize-only
mode, which does not construct `PaperFeedRuntime` and previously bypassed its
startup policy validation.

No trigger monitoring, execution, signing, wallet access, or broadcasting was
added.
