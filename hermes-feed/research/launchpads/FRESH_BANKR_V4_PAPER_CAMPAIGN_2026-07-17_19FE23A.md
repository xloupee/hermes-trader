# Fresh Bankr V4 paper campaign at `19fe23a`

Evidence date: 2026-07-17 UTC. Network: Robinhood Chain mainnet, chain ID
`4663`. Exact source commit:
`19fe23af9b0a8283dfed6943aa8c4ed53ccb09d4`. Machine-readable companion:
`FRESH_BANKR_V4_PAPER_CAMPAIGN_2026-07-17_19FE23A.json`.

The immutable runtime artifact root is:

```text
/Users/kennethjiang/.codex/worktrees/c20e/hermes-trader 2/hermes-feed/.runtime/paper-campaign-bankr-v4-19fe23a-20260717T085018Z
```

## Decision

The campaign completed three sequential, independent, non-overlapping
five-minute live-acquisition windows. All three completion manifests were
published, all three fresh startup snapshots validated chain `4663` and the
41 expected pins, and the authoritative campaign accepted three windows while
excluding zero partial windows. The completed root contains no `.partial`
file, FIFO, or symlink. The pinned readiness and evidence-report binaries
regenerated both authoritative outputs byte-for-byte.

This is paper-only evidence. Every launchpad remains
`paper_evidence_ready=false`, `authorizes_canary=false`, and
`execution_eligible=false`. No wallet, key, keystore, signing, broadcast,
execution, canary, deployment, server, or Droplet operation occurred. Nothing
in this report authorizes any of those actions.

## Provenance

| Item | Keccak-256 |
| --- | --- |
| Expected pins | `0x3a192547af7a76a47d5de1b8102da0dc0bb77b7047a53c855b7d264aefa1a34c` |
| Campaign lock | `0x2235dd0d9323d51249f5104c22dcb6e5f7aa26d5fb17b56f58a628839d9eaa7e` |
| Authoritative readiness | `0xaedd41260c786d488d386a10b0843e479a6615745ba2d6b36543e2d2bec6f4e0` |
| Evidence report | `0x1f123cefa9d91019319ee43eca721f76fa1d9b428601b2a0e8d96637d02ca8a7` |
| `hermes-feed` | `0xd383c3410183a8b632079031b9bcaadea71f4c59bba8d610cfb24de84afb33e5` |
| Paper observer/finalizer | `0xe558e925d9f4926f2804b6e6834a6b437427edb122737dc8417376b6b1ea0fbc` |
| Reconciler | `0x534e1192782983d8def097318bf9c98b856f52ccbf0d5302eb112ae9f745a7b8` |
| Chain-head sampler | `0x319fbb7cf811925040c5607f321d407d2b2fa4845e9c5a8f2638352b31d02d93` |
| Readiness evaluator | `0x062736569de776a557fe7e96c91082081af1c1aeb3053b289304de4c9ea0d4bc` |
| Evidence-report generator | `0xd525be3d39326a46349a608634f370976809d1687268c6bd51e3f5d0976cdf99` |
| Pin-snapshot tool | `0xb6dadd7761e21d8b12f171674908d7d74825be2e807dc12d7d222b311267502f` |
| Local FIFO runner | `0xb065b10aa70b8caaa586298346ac06d11f27871dd6c35e0e6f3249e876356fa3` |

The current outer campaign runner hashes to
`0x11c708fbb10e0f9d77659517762d035c4e1d32d271e5fde8ed275b582b24a7c1`.
That is a current-source check; unlike the inner runner, it is not a field in
the frozen campaign lock.

## Acquisition topology and window independence

Each window used the required split-output topology:

```text
hermes-feed probe --record private-mode-0600-raw-FIFO
  -> tee raw-feed.jsonl -> private-mode-0600-observer-FIFO
  -> hermes-launchpad-paper --acquisition live --input -
```

Probe stdout contained metrics only. Raw replayable frames traveled through
the FIFO and tee to both the immutable raw file and observer stdin. Receipt
and event reconciliation ran after EOF against independently sampled start
and cutoff boundaries, then the finalizer produced paper trade plans. The
completion manifest was written last.

| Window | Snapshot | Start and scored coverage | Snapshot report / manifest hash |
| --- | --- | --- | --- |
| A | L2 `11989209`, `0x13b5369d198fa5383f94e562ae33303f54ae41995c592b6a79129c487745a5d4`; L1 `25551322`; timestamp `1784278236` | start `11989287`, coverage `[11989288,11992259]`, cutoff `0xc83ecd6a50619e4b8cb87c1ae1b9588b6e611167568bf22c96ce78a9dbf6f349` | `0x850c37db1f47fb5098e11325051c1d1229700aba2bc449f6179ead649fcbc73f` / `0x92fe468244a2c7ea81342d064a22532df4d28437020e2d685cac2a4529d50b60` |
| B | L2 `11992443`, `0x9d885eca5ecea1545c855406c866efdfaf7207d5263c9c37d54593e93ea4d853`; L1 `25551348`; timestamp `1784278561` | start `11992510`, coverage `[11992511,11995483]`, cutoff `0x77ac3ef4093eb4ff58a98dc4de0f96ee30f47e592a58a5757221ffbc3b60410c` | `0x7dd68f27022f2b4cac8de1d6a645a95110e59fce2b774d3562ac50c180464c91` / `0xfacd3330de8fdb18be1c7424780368979bf69ab54224d2d706ad84a570b83a2c` |
| C | L2 `11995632`, `0x4ef31b56346c3e2740d0c43c0a73ee1c91e86d019b27557e04ed184c40de87a5`; L1 `25551376`; timestamp `1784278882` | start `11995705`, coverage `[11995706,11998678]`, cutoff `0xbf430d617fc15a8a5e26cb8a6c7fb119398a1c5c306c64804efbd74916a6248e` | `0x26128542bfad91abc9255cd863aecb3d59bd386a4ea487232b040ff2df1feb2d` / `0xe506c8cbcab005569014ed0461d37fe222a1e8dbc223578e832b0df4b5323300` |

The scored ranges are disjoint and ordered. Snapshot-to-start gaps were 78,
67, and 73 L2 blocks, below the allowed maximum of 500.

## Aggregate result

`Eligible` means quote-eligible confirmed paper evidence, never execution
eligibility. Latencies are observation latencies in nanoseconds. Error columns
are false positives, detector misses, coverage misses, identity mismatches,
direction mismatches, prediction mismatches, and independent-quote
mismatches, in that order.

| Launchpad | Confirmed / eligible | Latency p50 / p95 / p99 | FP / miss / coverage / identity / direction / prediction / quote | Entry / exit plans | Profile observations |
| --- | ---: | ---: | ---: | ---: | --- |
| Bow | `0 / 0` | `null / null / null` | `0 / 0 / 0 / 0 / 0 / 0 / 0` | `0 / 0` | payable `0`; zero-initial-buy `0` |
| LaunchHood V3 | `7 / 7` | `323000 / 1317000 / 1317000` | `0 / 0 / 0 / 0 / 0 / 0 / 0` | `7 / 7` | embedded initial buy `7` |
| Clanker | `4 / 4` | `459000 / 607000 / 607000` | `0 / 0 / 0 / 0 / 0 / 0 / 0` | `4 / 4` | extensionless `2`; pinned extension `2` |
| Bankr/Doppler | `0 / 0` | `null / null / null` | `0 / 6 / 0 / 0 / 0 / 0 / 0` | `0 / 0` | V1/V2/V3/V4/direct-Airlock/ERC-7579 all `0` |
| Pons | `49 / 1` | `357000 / 904000 / 1277000` | `0 / 0 / 0 / 2 / 0 / 0 / 0` | `1 / 1` | current generation `1` |
| hood.fun | `0 / 0` | `null / null / null` | `0 / 0 / 0 / 0 / 0 / 0 / 0` | `0 / 0` | current curve `0` |

LaunchHood's seven and Pons's one simulated immediate round-trip ratios are
`0x2649`, or 9801 basis points. Clanker's four are `0x351`, or 849 basis
points. These are paper-quote ratios `exit_output * 10000 / entry_input`, not
realized profit or loss. All plans used the observer's independent pinned
quote and sizing path; no plan was signed or submitted.

Per-window confirmed counts were LaunchHood `1/2/4`, Clanker `2/1/1`, Pons
`17/17/15`, and zero for Bow, Bankr, and Hood. Bankr detector misses were
`1/1/4`. All false-positive counters were zero in every aggregate. Pons's
aggregate identity count of two is two missing prediction fields, token and
pool, on the single transaction
`0xf40cc1543052e78c2c2fbb2dd6561dd14905ffc3bae0266e397d7c3ce6a36583`;
it is not two transactions and does not change the zero false-positive,
direction-mismatch, prediction-mismatch, or quote-mismatch counters.

## Bankr misses: strict failure, not broadening authority

The six successful-receipt Airlock ground-truth events below had no observer
claim and were rejected with the exact blocker
`bankr_strict_quote:Airlock create calldata is malformed or not the reviewed standard profile`:

```text
window-a 11991651 0x4c910a52338472b365dadec2dd0bd24443f189396674750d74e93226e8e36fd6
window-b 11994560 0xd1641af3d4bfc5edb4efc118d0e3de7370c3d7385c5be74d138e2317485a582b
window-c 11996139 0xd513a87cc85a141ef88fb3ca7a0ce618223930a2585ff7ca9d0400ff2a2eee9f
window-c 11996520 0x0ac9b544c6f5e345bbfc4697eca18f185592f4fb935583829160995cf919e9d6
window-c 11995775 0xc62d71574bc598c026a8f44d4aacac2a599fb61edbb41a117d0ed19e2e1c8a51
window-c 11998586 0x227b398f6f2e4d61ba30c4d32a163d359dae1e073f4fcbbd94c1641117526ebc
```

These rows are evidence that fresh Bankr activity exists outside the currently
reviewed strict profiles. They are not evidence that the rows share one safe
profile, are CurveTicksV4, or should be accepted by a global selector,
fallback parser, relaxed identity check, or generic Airlock rule. No existing
profile recorded an observation, including CurveTicksV4. The correct next
step is an independent calldata, envelope, account/delegation, receipt, and
negative-test audit before proposing any narrowly bounded profile change.

## Readiness and limitations

The policy requires at least 100 quote-eligible confirmations, at least 10 in
each supported profile envelope, three complete non-overlapping windows, and
zero error counters. Only the window-count condition passed universally.
Bankr additionally fails on six detector misses. Pons additionally fails on
the two field-level missing identity predictions on the single transaction
identified above. Therefore every readiness, canary, and execution boolean
remains false.

This campaign is a fifteen-minute activity sample, not proof of absence for
Bow or Hood and not sufficient coverage for any launchpad. A zero false-
positive count only applies to the observed windows. Paper slippage, sizing,
entry, exit, and immediate-round-trip values are simulations under pinned
state; they do not establish fillability, realized return, mempool ordering,
or execution safety. Promotion remains out of scope until the evidence gates
pass and separate explicit authority is provided.
