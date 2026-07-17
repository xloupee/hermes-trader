# Pons fresh1 identity eligibility audit

## Scope

This audit uses the finalized V3 replay evidence from
`paper-session-20260716-pins39-fresh1`. It changes paper metrics only. It does
not enable execution, signing, broadcasting, wallet access, or server work.

## Reproduced evidence

The independent reconciliation collector produced 40 Pons receipt rows:

- all 40 target the pinned legacy factory and carry `pons_generation: legacy`;
- all 40 carry `quote_status: not_applicable` and the exact blocker
  `legacy_pons_generation_is_discovery_only_without_strict_receipt_profile`;
- 37 are canonical protocol-event confirmations inside L2 blocks
  11,715,615 through 11,718,597;
- three successful observer claims are outside that coverage interval;
- the in-window result is 37 confirmed observations, zero false positives,
  zero detector misses, and 37 action-prediction matches.

Before this change, each of the 37 confirmed legacy observations contributed
one missing token prediction and one missing pool prediction. The resulting 74
`identity_mismatches` conflicted with the same readiness row's explicit
`current_generation: 0` profile-envelope count.

## Protocol decision

Legacy launches remain ground truth for detector coverage and action
classification. They are not identity-prediction or quote eligible:

- the observer intentionally emits `UnresolvedUntilReceipt` and
  `DiscoveryOnly` for this generation;
- the strict receipt quoter only admits the separately reviewed current
  generation;
- no normalized legacy token creation code or constructor encoding is pinned
  in the production prediction profile;
- pool identity is derivable from a token only after token identity is known,
  so V3 CREATE2 alone does not close the pre-receipt gap.

The verified legacy source and its `predictTokenAddress` view are useful
discovery evidence, but source narrative is not a production prediction pin.
Implementing prediction still requires extracting and pinning the exact token
init-code construction and proving multiple calldata-to-receipt pairs plus
salt/factory/constructor negatives.

Accordingly, token/pool comparison is now inapplicable only when all four
independent evidence fields agree: Pons launchpad, legacy generation,
not-applicable quote status, and the exact legacy discovery blocker. Current or
ambiguous evidence continues to fail closed and accrues missing identity
counts. Action prediction remains eligible for legacy observations.

## Post-change replay

Re-finalizing the unchanged fresh1 observer and V3 replay evidence produces:

- 37 confirmed observations;
- 37 quote-not-applicable observations;
- 37/37 action-prediction matches;
- zero token/pool prediction-eligible legacy observations;
- zero identity mismatches;
- zero current-generation and zero quote-eligible observations.

This does not make Pons canary-ready. The readiness sample contains no
current-generation strict quote or prediction evidence, so the existing
profile and sample gates remain closed.
