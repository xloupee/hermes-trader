# Hermes NOXA runtime architecture

## Fast path

```text
Nitro feed transaction at L1 block B
    -> strict canonical full launchToken decode
    -> cached factory/DEX configuration and pinned creation code
    -> CREATE2 token + V3 pool prediction
    -> deterministic mint + launcher initial-buy replay
    -> cached restriction policy and exact local quote
    -> pinned SwapRouter02 plan
    -> encrypted-keystore signature in memory
    -> arm exact bytes for L1 block B + 1
    -> contiguous feed header B + 1
    -> mark nonce submitted before network await
    -> one direct sequencer request
    -> bounded asynchronous trade reconciliation queue
    -> independent asynchronous launch-receipt proof
```

NOXA blocks public pool buys in the launch L1 block. Receipt hydration therefore
cannot be the decision gate. Startup pins the launch and V3 factory runtimes,
their embedded creation-code hashes, launch configuration, DEX configuration,
and launch fee. Candidate-time prediction, quoting, risk checks, construction,
and signing are synchronous and RPC-free. The signed bytes wait only for the
next contiguous feed header at `B + 1`; receipt proof and reconciliation remain
background work.

## Watched-wallet copy strategy

`hermes-noxa-runtime --strategy copy` reuses the same Nitro decoder, boundary
gate, nonce lease, risk ledger, signer, sequencer client, and receipt
reconciliation path. It is deliberately opt-in and requires leaders supplied
with repeatable `--watch-wallet` flags or a private `--watch-wallet-file`.
`--copy-token` remains an optional startup bootstrap, but is no longer required.
Unknown tokens are suppressed, validated asynchronously at one pinned L2 block,
and admitted only after their bytecode, token-reported NOXA factory/pair/pool/fee,
canonical CREATE2 V3 pool identity, pool bytecode, and non-zero liquidity match.
Verified launch receipts populate the same pool-bound registry without waiting
for a later manual token-list update.

The copy hot path considers canonical `SwapRouter02.exactInputSingle` calls and
strict single-leg `0x4d819a2a` Robinhood aggregator calls signed by a watched
wallet. Aggregator native-ETH buys are normalized to WETH input and token sells
to WETH output. The follower always submits through the pinned SwapRouter02,
never the upgradeable aggregator. Redirected recipients, inconsistent native
value flags, multi-leg or extended aggregator routes, non-WETH pairs, wrong
pool addresses or fees, non-zero direct-router price limits, zero limits,
unvalidated tokens, and non-Robinhood transaction chain IDs are rejected. Entry size is
the follower's independent fixed cap, never the leader's size. The follower's
minimum output preserves the leader's calldata limit price proportionally. An
exit is admitted only for the matching reconciled follower position and sells
that complete position at the leader's proportional minimum price.
Filtering is ordered for minimum work: router address, four-byte selector,
signer recovery, watched-wallet membership, then strict full ABI decoding.
Unwatched router or aggregator traffic never pays the full ABI decode cost.

This fastest path intentionally does not add an RPC quote before signing. Paper
mode records that the fill basis is the leader's limit-price floor. Signed copy
broadcast therefore additionally requires
`--copy-trust-leader-limit-price`; without that explicit choice it fails closed.

Signed entry reconciliation creates an exact, full-position token allowance;
the runtime waits for that approval receipt and then for a watched-wallet exit.
It never converts copy mode into an automatic timed round trip. The default
two-trigger session cap admits at most one entry and one full exit. Mainnet
broadcast still requires the separate exact canary approval token.

The retained historical oracle at transaction `0xc629…e418` exactly matches the
real token, pool, restriction end, initial-buy amount, final V3 state, and next
quote. A full 60,142-event scan then selected 30 evenly spaced receipts from
each token-orientation/zero-or-nonzero-initial-buy category. Of those 120 cases,
107 had archival state available and matched every predicted field and quote
exactly; the public RPC reported a missing historical trie node for the other
13. There were zero verifiable mismatches, with 26 or 27 exact cases in every
category. In an optimized 10,000-iteration run, prediction plus both V3 quotes
averaged 71,600 ns per iteration in the latest run on the current host. The same
audit held nonce 7 at the launch boundary, released it exactly once at `B + 1`,
and reconciled the predicted fill into nonce 8, open exposure, and a position.

## Fail-closed invariants

- A feed gap, reordering, L1 regression, expired condition window, or timestamp
  regression permanently cancels an unsubmitted candidate.
- Startup pins an EOA factory owner, then revalidates fee, enablement, owner,
  configurations, runtimes, and creation-code hashes after feed warmup. Any
  later non-launch transaction sent directly to the factory invalidates the
  cache and stops the runtime before another candidate can be armed.
- Duplicate headers cannot release transaction bytes twice.
- Only one nonce and one risk reservation may be active.
- The nonce is marked submitted before the direct sequencer await begins.
- Accepted, already-known, rate-limited, malformed, transport-ambiguous, and
  HTTP-error outcomes retain the nonce until reconciliation.
- Only explicit `BlockNumberMin` failure or a definitive JSON-RPC rejection may
  release the nonce without receipt reconciliation.
- A saturated reconciliation queue halts new entries while preserving the
  pending transaction hash.
- Session-loss breach halts entries; risk-reducing paper exits remain allowed.
- Successful signed receipts derive the fill from output-token `Transfer` logs
  to the signer, then persist token amount and WETH cost basis.
- Signed exits must sell the complete recorded token position back to WETH;
  reconciliation releases its cost basis from cumulative exposure. Loss-limit
  breach blocks later entries but does not block this risk-reducing exit.
- NOXA documents a standard ERC-20 interface but no ERC-2612 permit path, and
  the pinned factory runtime contains `approve(address,uint256)` without the
  standard permit selectors. The optional round-trip canary therefore signs an
  exact router approval only after entry reconciliation, waits for that receipt,
  then signs the full-position exit with an explicit minimum WETH output.
- Entry preflight requires the router's WETH allowance to equal the capped
  input exactly; unlimited or excess allowances are rejected.

## Component map

- `boundary_gate.rs`: exact-once feed boundary transition.
- `noxa_predict.rs`: cached configs + feed calldata -> deterministic token,
  pool, limits, post-launch V3 state, and quote.
- `noxa_candidate.rs`: predicted launch + cached quote -> pinned unsigned plan.
- `noxa_verifier.rs`: asynchronous receipt proof and differential oracle.
- `signer.rs`: encrypted keystore loading and address-bound in-memory signing.
- `trading_runtime.rs`: nonce/risk/sign/arm/submission/reconciliation state.
- `hot_path.rs`: one direct sequencer request and bounded reconciliation handoff.
- `paper_runtime.rs`: broadcast-free entry, position, exit, loss, and nonce model.
- `copy_policy.rs`: watched-wallet/token allowlists, fixed follower sizing,
  proportional limit-price enforcement, trigger cap, and full-position exits.
- `hermes-paper-scenario`: deterministic executable paper runner.
- `hermes-keystore-check`: public-address-only secure provisioning check.
- `hermes-noxa-runtime`: live feed loop for paper, signed dry-run, and explicitly
  approved capped broadcast modes.

## External gates

- The canonical NOXA factory currently reports `launchEnabled=false`.
- Robinhood and NOXA do not currently publish canonical chain-46630 WETH,
  SwapRouter02, NOXA factory, or usable testnet launch liquidity.
- Mainnet broadcast remains disabled until a separately approved capped canary.
