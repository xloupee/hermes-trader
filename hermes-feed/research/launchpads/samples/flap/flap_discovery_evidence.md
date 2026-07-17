# Flap canonical discovery samples

## Disposition

This is launchpad-wide, discovery-only evidence for Flap on Robinhood Chain
mainnet (`chain_id = 4663`). In the exact finalized 250,000-block window
`12,200,729..12,450,728`, the canonical Portal emitted 3,343 valid
`TokenCreated` receipt logs for 3,343 unique tokens:

- 3,242 were direct Portal calls;
- 101 were VaultPortal-origin calls whose canonical `TokenCreated` event was
  still emitted by Portal;
- 3,343 were confirmed, with 0 false positives, 0 provider-relative in-range
  misses, 0 strict-decode misses, and 0 action mismatches;
- 22,516 `TokenBought` and 13,808 `TokenSold` logs were counted only as
  controls. Zero trade logs were substituted for launch ground truth.

This evidence admits no Vault selector, prediction, quote, readiness,
execution, or promotion path. Every claim has `quote_eligible = false`, and
entry, exit, and slippage outcomes are null because none were attempted.

Machine evidence:
[`flap_discovery_evidence.json`](./flap_discovery_evidence.json), SHA-256
`df28947fa1deb02d8d010d5bc070577a56af4b0ab5e587e1d16e0b5af161d186`.

## Boundaries and provenance

| Field | Value |
|---|---|
| Repository source SHA | `3829a7b2dccb2c651c85a920e19c2f705607ab6d` |
| Branch | `codex/samples-flap-discovery` |
| Public RPC | `https://rpc.mainnet.chain.robinhood.com` |
| Concurrency | `1` |
| Start block | `12,200,729` |
| Start block hash | `0x105142014d12422cb5eb93cc9c6585770ebcf10939492e2691a8382d4ea44573` |
| Start timestamp | `2026-07-17T14:44:41Z` |
| Finalized end block | `12,450,728` |
| End block hash | `0xa67f945c14e517799212eea5cf960cf81757c30d4a42a139b961da1ed7bbbaf8` |
| End timestamp | `2026-07-17T21:42:22Z` |
| Block count | `250,000` (the requested cap) |
| RPC requests | `6,826`, serial, with bounded retry/backoff |

The end block hash was fetched again after transaction/receipt resolution and
matched exactly. Portal and VaultPortal proxy runtime hashes, EIP-1967
implementation addresses, implementation runtime hashes, and versions matched
their source pins both before and after the scan at the same finalized block.
Any mismatch aborts the collector before output publication.

## Ground-truth and classification rules

The sole launch oracle was a successful receipt log with all of:

1. emitter `0x26605f322f7ff986f381bb9a6e3f5dab0beaeb09` (Portal);
2. topic 0
   `0x504e7f360b2e5fe33cbaaae4c593bc55305328341bf79009e43e0e3b7f699603`
   (`TokenCreated`);
3. exact transaction hash, block hash, and log-index identity present once in
   the fetched successful receipt;
4. `removed = false`; and
5. strict static decoding with nonzero creator and token.

Origin classification uses the outer transaction destination, not the event
emitter. `transaction.to == Portal` is direct Portal origin;
`transaction.to == VaultPortal` is VaultPortal origin; anything else is an
action mismatch and remains fail-closed. This separation matters because all
101 VaultPortal-origin launches emitted their canonical event from Portal, not
VaultPortal. Their decoded creator was VaultPortal in all 101 cases. For all
3,242 direct claims, decoded creator matched the outer transaction sender.

The observed call distribution was:

| Origin | Observed selector | Count | Evidence classification |
|---|---:|---:|---|
| Direct Portal | `0x8cb5772c` | 3,097 | direct Tax V3 creation; discovery only |
| Direct Portal | `0x2e2fdbd9` | 145 | direct standard V5 creation; discovery only |
| VaultPortal | `0x1b806220` | 101 | VaultPortal-origin canonical event; observed, explicitly not admitted |

Selectors are recorded to bind the observed action and detect mismatches. This
does not add them to any registry or allowlist. No source, config, runtime,
manifest, or existing research file was changed.

## Results and mismatch accounting

| Metric | Result |
|---|---:|
| Canonical claims | 3,343 |
| Confirmed | 3,343 |
| False positives | 0 |
| Ground-truth misses | 0 |
| Strict-decode misses | 0 |
| Action mismatches | 0 |
| Direct Portal origin | 3,242 |
| VaultPortal origin | 101 |
| Unique tokens | 3,343 |
| Duplicate event identities | 0 |
| Duplicate token claims | 0 |
| `TokenBought` controls | 22,516 |
| `TokenSold` controls | 13,808 |
| Trade-event substitutions | 0 |

“Ground-truth misses” is deliberately narrow: canonical `TokenCreated` logs
returned by the endpoint within the exact `eth_getLogs` range but omitted from
the claim set. It is zero by complete one-to-one reconciliation. It is not a
claim that this single RPC provider independently proves its own historical
completeness.

## Reproduction and validation

Collection command (from repository root):

```sh
FLAP_SOURCE_SHA=$(git rev-parse HEAD) \
  node hermes-feed/research/launchpads/samples/flap/collect_flap_discovery.mjs \
  hermes-feed/research/launchpads/samples/flap/flap_discovery_evidence.json
```

Collection result:

```text
canonical_token_created_claims=3343 confirmed=3343 false_positives=0
ground_truth_misses=0 action_mismatches=0 direct_portal_origin=3242
vault_portal_origin=101 token_bought_controls=22516 token_sold_controls=13808
rpc_request_count=6826
```

Targeted validation commands:

```sh
node --check hermes-feed/research/launchpads/samples/flap/collect_flap_discovery.mjs
jq empty hermes-feed/research/launchpads/samples/flap/flap_discovery_evidence.json
jq -e '<integrity assertions>' hermes-feed/research/launchpads/samples/flap/flap_discovery_evidence.json
git diff --check
git status --short
```

The integrity assertions require the exact block cap, stable/matching start and
end pins, confirmed plus false-positive equality, direct plus vault plus action
mismatch equality, all unique receipt identities, zero trade substitutions,
zero quote/entry/exit/slippage activity, and per-claim receipt/event/eligibility
invariants.

## Assumptions, risks, and unresolved questions

- Assumption: the public Robinhood RPC returns a complete historical log set
  for the requested finalized range. The endpoint was internally consistent,
  but no second provider was used.
- Risk: Portal and VaultPortal are upgradeable. These samples are bound to the
  exact finalized boundary and pin set; they do not authorize future behavior.
- Risk: selector labels describe only the observed calls and pinned research.
  They are not full ABI/profile verification and do not admit any action.
- Unresolved: independent RPC or Blockscout one-to-one completeness replay.
- Unresolved: quote asset, entry, exit, slippage, migration, and execution
  semantics. They were intentionally excluded and remain unavailable.

## Integration instructions

Integration may consume the JSON only as immutable discovery evidence. Preserve
the two origin classifications while keeping Portal `TokenCreated` as the only
launch ground truth. Do not create launch candidates from `TokenBought` or
`TokenSold`. Do not copy the observed Vault selector into a registry, infer a
prediction, normalize a quote, mark readiness, construct an execution path, or
promote Flap from this artifact. Any such work requires a separately reviewed
task and fresh pin-bound evidence.
