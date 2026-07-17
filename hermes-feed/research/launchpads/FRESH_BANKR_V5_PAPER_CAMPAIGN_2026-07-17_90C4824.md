# Fresh Bankr V5 paper campaign at `90c4824`

Evidence date: 2026-07-17 UTC. Network: Robinhood Chain mainnet, chain ID
`4663`. Exact source commit:
`90c4824bec5dc987259ce4f81858940e247fa81c`. Machine-readable companion:
`FRESH_BANKR_V5_PAPER_CAMPAIGN_2026-07-17_90C4824.json`.

The immutable runtime artifact root is:

```text
/Users/kennethjiang/.codex/worktrees/hermes-bankr-v5-fresh-campaign/hermes-feed/.runtime/paper-campaign-bankr-v5-90c4824-20260717T095215Z
```

## Decision

The campaign completed three sequential, independent, non-overlapping
five-minute live-acquisition windows. All three completion manifests were
published, all three fresh startup snapshots validated chain `4663` and all
41 expected pins, and the authoritative campaign accepted three windows while
excluding zero partial windows. The completed root contains no `.partial`
file, FIFO, or symlink. The pinned readiness and evidence-report binaries
regenerated both authoritative outputs byte-for-byte.

Two fresh `CurveTicksV5` Bankr launches and two `CurveTicksV4` launches were
detected, independently quoted, and finalized into non-executable paper plans.
One additional successful Bankr ground-truth launch was present in the raw feed
but failed closed because its top-level target is neither exact EntryPoint nor
direct Airlock. That new wrapper/account boundary is not evidence for
broadening V5.

This is paper-only evidence. Every launchpad remains
`paper_evidence_ready=false`, `authorizes_canary=false`, and
`execution_eligible=false`. No wallet, key, keystore, signing, broadcast,
execution, canary, deployment, server, or Droplet operation occurred. Nothing
in this report authorizes any of those actions.

## Provenance

| Item | Keccak-256 |
| --- | --- |
| Expected pins | `0x3a192547af7a76a47d5de1b8102da0dc0bb77b7047a53c855b7d264aefa1a34c` |
| Campaign lock | `0xaa14ae71e8dd8c7ce248f557d769682dddcbc02cf1372cfeaee2d4b931a41783` |
| Authoritative readiness | `0xc6c1e044fc7b578176053de5700f89efc162c6157159228e2bbbb5ebc50462de` |
| Evidence report | `0x2d8f0163f96cb8452c8f32dd97d53a2de8ac1429630911f21459eb2ea52bed18` |
| `hermes-feed` | `0xc62a2c74a345490b69f77c6f7f214bd4f68dce913003dd8d7ec56a2aa2610fbc` |
| Paper observer/finalizer | `0x3a2f0b23b8bc09aa09a897043271860e8c9efcbb322ed02530a1aa9c43cbe0f5` |
| Reconciler | `0xd0d5fad2cae56341a985d8b0025a4ca34028ebc9647ec5b184388afeb5674efe` |
| Chain-head sampler | `0x99fd000e1a263541e7263d865d9b6ed1888f9268af30ea5f56b65f3ea14708b9` |
| Readiness evaluator | `0xcb91abd9211b859f0f80c5aa0d73c9ce7d473800d82778e901b0428252e2b493` |
| Evidence-report generator | `0x811632709612570d6823d0a68fb9b099114c8111d2785ccfa69572b8b157a84a` |
| Pin-snapshot tool | `0x6374470fa2e8a5fbc651e86d0fd3f84a4dfe1ab11c1a1c42d9cfe6ae07de6516` |
| Local FIFO runner | `0xb065b10aa70b8caaa586298346ac06d11f27871dd6c35e0e6f3249e876356fa3` |

The current outer campaign runner hashes to
`0x11c708fbb10e0f9d77659517762d035c4e1d32d271e5fde8ed275b582b24a7c1`.
That is a current-source check; unlike the inner runner, it is not a field in
the frozen campaign lock.

## Acquisition topology and window integrity

Each window used the required split-output topology:

```text
hermes-feed probe --record private-mode-0600-raw-FIFO
  -> tee raw-feed.jsonl -> private-mode-0600-observer-FIFO
  -> hermes-launchpad-paper --acquisition live --input -
```

Probe stdout contained metrics only. Raw replayable frames traveled through
the FIFO and tee to both the immutable raw file and observer stdin. Receipt
and event reconciliation ran after EOF against independently sampled start and
cutoff boundaries, then the finalizer produced paper trade plans. Each
completion manifest was written last.

| Window | Snapshot | Start and scored coverage | Continuity | Snapshot report / manifest hash |
| --- | --- | --- | --- | --- |
| A | L2 `12026003`, `0x6a4a04533e754671db3a8a0e810a1b313631b164d25e7d8e6cd4cb310db742ea`; L1 `25551628`; timestamp `1784281934` | start `12026076`; coverage `[12026077,12029051]`; cutoff `0xddaffc19c2dfe160f223c7612d1147c834555972195ee27382e871ce1d8d1207` | sequence `12024910..12029063`; reconnect/gap/missing/reorder `0/0/0/0` | `0xcbdc93612ace76e8d44c0a94aba04e1d165ff016fd6e6d3d4536d285c6ff5c55` / `0x29bcc9395d7a7b7ef441c84b61f8d1a94bf18e8fb01d80c44bdd861a4ceac76b` |
| B | L2 `12029350`, `0x28580c1078b43abc142359c0f6aaa424c7bd98c4a21786a30c190b1337b03fc9`; L1 `25551655`; timestamp `1784282270` | start `12029422`; coverage `[12029423,12032398]`; cutoff `0x8f73f575a1a371c37d38facdb3aee6a6f42d1c5cb2289fab81d0853b2c6dfaf3` | sequence `12027930..12032408`; reconnect/gap/missing/reorder `0/0/0/0` | `0xafcaac0ec3716892b2ffd21c9d267fe1fe055b66186123f83a3dd493ddef36c6` / `0x1d92845ef529e780d934003295b999a447157ea791db309c4de686cdf4b4f530` |
| C | L2 `12032702`, `0xe3edadc3a4c7542b81d84a27d2457dde52050876c0be384bd27a776b904cb4d8`; L1 `25551684`; timestamp `1784282607` | start `12032769`; coverage `[12032770,12035749]`; cutoff `0x6fb47f6920d8dd129d09a59a04b0012f7a543b39ecb8712acb96056e5175e9cd` | sequence `12031615..12035760`; reconnect/gap/missing/reorder `0/0/0/0` | `0xe33b69e721ff4ce62230a74110bb10a4f3dfd4c93e7c9b88a911e1f304045bc9` / `0x66a39a94a17587061ae6a94cac485d1fe909e2c1107ee26a57bf7b3a27a44370` |

The scored ranges are strictly ordered and disjoint. Snapshot-to-start gaps
were 73, 72, and 67 L2 blocks, below the allowed maximum of 500. Every fresh
snapshot used 77 logical RPC requests and 77 HTTP attempts, with zero retries,
rate limits, server errors, or transport errors.

The companion JSON records every manifest-bound raw-feed, probe-metrics,
observer, reconciliation, finalized, start-anchor, cutoff-anchor, and observed
snapshot content hash.

## Aggregate result

`Eligible` means quote-eligible confirmed paper evidence, never execution
eligibility. Latencies are observation latencies in nanoseconds. Error columns
are false positives, detector misses, coverage misses, identity mismatches,
direction mismatches, prediction mismatches, and independent-quote mismatches,
in that order.

| Launchpad | Confirmed / eligible | Latency p50 / p95 / p99 | FP / miss / coverage / identity / direction / prediction / quote | Entry / exit plans | Profile observations |
| --- | ---: | ---: | ---: | ---: | --- |
| Bow | `2 / 2` | `477000 / 663000 / 663000` | `0 / 0 / 0 / 0 / 0 / 0 / 0` | `2 / 2` | zero initial buy `1`; payable initial buy `1` |
| LaunchHood V3 | `8 / 8` | `336000 / 648000 / 648000` | `0 / 0 / 0 / 0 / 0 / 0 / 0` | `8 / 8` | embedded initial buy `8` |
| Clanker | `9 / 9` | `347000 / 876000 / 876000` | `0 / 0 / 0 / 0 / 0 / 0 / 0` | `9 / 9` | extensionless `0`; pinned extension `9` |
| Bankr/Doppler | `4 / 4` | `545000 / 645000 / 645000` | `0 / 1 / 0 / 0 / 0 / 0 / 0` | `4 / 4` | V4 `2`; V5 `2`; ERC-7579 `4`; other reviewed profiles `0` |
| Pons | `42 / 3` | `333000 / 512000 / 625000` | `0 / 0 / 0 / 6 / 0 / 0 / 0` | `3 / 3` | current generation `3` |
| hood.fun | `1 / 1` | `334000 / 334000 / 334000` | `0 / 0 / 0 / 0 / 0 / 0 / 0` | `1 / 1` | current curve `1` |

Bow, LaunchHood, Pons, and Hood simulated immediate round-trip ratios are
`0x2649`, or 9801 basis points. Clanker's ratio is `0x351`, or 849 basis
points. Bankr's ratio is `0x61`, or 97 basis points. These are paper-quote
ratios `exit_output * 10000 / entry_input`, not realized profit or loss. All
plans used independent pinned quote and sizing paths; no plan was signed or
submitted.

## Bankr profile result and new strict miss

The four in-range, independently confirmed and quoted Bankr transactions are:

| Window | Transaction | L2 block | Exact profile | Envelope |
| --- | --- | ---: | --- | --- |
| A | `0xc5d8068c27a4afe7335f5bf31cbf764a8b561670a50b88e6b3e234f8bec20cfd` | `12028564` | `curve_ticks_v5` | `erc7579` |
| B | `0x58ac1df3ffda18323bd83fc792f4437e21c83024edbf771a5851425ae2fe7b7f` | `12032244` | `curve_ticks_v4` | `erc7579` |
| B | `0x47ff0dd828b457e592cae6d257667fb45c7e9c6d4deb2e51eb9573d028227ffd` | `12029696` | `curve_ticks_v5` | `erc7579` |
| C | `0xc81eaccbf690b7cbd92da46af3b43efb2d8849e78f9603a79c122b1f4bf6421c` | `12034155` | `curve_ticks_v4` | `erc7579` |

Thus both fresh V5 observations were detected, independently quoted, and
finalized. The six historical V5 proof transactions documented in
`BANKR_CURVE_TICKS_V5_FRESH_EVIDENCE_2026-07-17.md` are older fixed-block
fixtures outside these new campaign ranges; they cannot recur as new live
observations.

One new successful-receipt Airlock ground-truth event was raw-feed-present but
had no observer claim:

```text
window-c 12033710/2
0xd53c3d8d8c76fd5f367d3d229a45e1aef65c0cdb712d94421f311f97fe6dd563
from     0x426e36b803520760a792d74a7fa092b0c85c4d9a
target   0x2a71f10b41ff0882c7be2a5c0644722314976b42
selector 0x376d6552
value    0x0
```

The exact blocker was:

```text
bankr_strict_quote:receipt-block Bankr account identity proof failed: transaction is neither exact EntryPoint nor direct Airlock envelope
```

This is a correct fail-closed result. The transaction requires an independent,
narrowly bounded wrapper, account, delegation, calldata, receipt, and negative-
test audit. It does not authorize broadening V5, global selector dispatch,
fallback parsing, or a generic Airlock rule.

## Pons identity gate

Pons's aggregate identity count of six is six missing prediction fields, not
six transactions or six wrong predictions. Exactly three independently quoted
current-generation transactions each lack the observer's token and pool
prediction fields:

```text
window-b 0x603805cc5b1ffc04f410ba9481764c2d2eb7e1f568f66bac9b15c6949f78578b token,pool
window-b 0xcd92a3cce53c0e1d24a80f829799bd912feda584175c70da8d0c908ffa564d47 token,pool
window-c 0x6893ff469f43ea30c9b5ced4b5c4ad45d8a9135bc8b2c737a6d88253cdc04e2c token,pool
```

Pons still has zero false positives, direction mismatches, prediction
mismatches, and quote mismatches. The missing fields nevertheless keep the
readiness gate fail-closed.

## Readiness and limitations

The policy requires at least 100 quote-eligible confirmations, at least 10 in
each supported profile envelope, three complete non-overlapping windows, and
zero error counters. Only the window-count condition passed universally.
Bankr additionally fails on its new detector miss. Pons additionally fails on
the six field-level missing identity predictions. Therefore every readiness,
canary, and execution boolean remains false.

This campaign is a fifteen-minute activity sample, not sufficient coverage for
any launchpad. A zero false-positive count applies only to the observed
windows. Paper slippage, sizing, entry, exit, and immediate-round-trip values
are simulations under pinned state; they do not establish fillability,
realized return, mempool ordering, or execution safety. Promotion remains out
of scope until the evidence gates pass and separate explicit authority is
provided.

## Reproduction

Using the exact tuple that produced the campaign, both `cmp` commands exit `0`:

```sh
ROOT='/Users/kennethjiang/.codex/worktrees/hermes-bankr-v5-fresh-campaign/hermes-feed/.runtime/paper-campaign-bankr-v5-90c4824-20260717T095215Z'
BINDIR='/Users/kennethjiang/.codex/worktrees/hermes-bankr-v5-fresh-campaign/hermes-feed/.runtime/immutable-v5-tuple-90c4824-20260717T095021Z/bin'

"$BINDIR/hermes-launchpad-readiness" \
  --expected-self-keccak256 0xcb91abd9211b859f0f80c5aa0d73c9ce7d473800d82778e901b0428252e2b493 \
  --session-dir "$ROOT/windows/window-a" \
  --session-dir "$ROOT/windows/window-b" \
  --session-dir "$ROOT/windows/window-c" | \
cmp - "$ROOT/authoritative-readiness.jsonl"

"$BINDIR/hermes-launchpad-evidence-report" \
  --expected-self-keccak256 0x811632709612570d6823d0a68fb9b099114c8111d2785ccfa69572b8b157a84a \
  --campaign-lock "$ROOT/campaign-lock.json" \
  --snapshot-keccak256 0x6374470fa2e8a5fbc651e86d0fd3f84a4dfe1ab11c1a1c42d9cfe6ae07de6516 \
  --local-runner-keccak256 0xb065b10aa70b8caaa586298346ac06d11f27871dd6c35e0e6f3249e876356fa3 \
  --readiness-output "$ROOT/authoritative-readiness.jsonl" \
  --expected-pins "$ROOT/expected-pins.locked.json" \
  --readiness-keccak256 0xcb91abd9211b859f0f80c5aa0d73c9ce7d473800d82778e901b0428252e2b493 \
  --session-dir "$ROOT/windows/window-a" \
  --session-dir "$ROOT/windows/window-b" \
  --session-dir "$ROOT/windows/window-c" | \
cmp - "$ROOT/launchpad-evidence-report.jsonl"
```

The completed root has exactly three session manifests and no partial, FIFO,
or symlink residue.
