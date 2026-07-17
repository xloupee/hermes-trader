# Post-Stonks fresh paper campaign: 2026-07-17 / `38fc684`

## Verdict

The campaign operator recorded a clean checkout at pushed source commit
`38fc684a67d648f7118ed3bdcf038422d866496a` before the isolated build. The
immutable runtime root does **not** encode or cryptographically bind that source
SHA, so it is a checkout observation rather than a claim proved by the runtime
artifacts. The runtime artifacts themselves prove three complete, sequential,
non-overlapping, paper-only five-minute windows. The campaign accepted all
three windows and excluded zero partial windows. Each window had a distinct
fresh startup snapshot, 41/41 expected pins validated, exactly one `connected`
followed by one `coverage_closed` continuity record, zero snapshot RPC errors,
and a completion manifest published only after reconciliation and finalization.

This sample does **not** support promotion. No launchpad is paper-evidence
ready, no canary is authorized, and no plan is execution eligible. The fresh
sample contains two Bankr detector misses, one Pons detector miss, no Bow or
Hood launches, no Clanker extensionless profile, and no Stonks V3 transaction.
The Stonks implementation therefore received **zero fresh campaign proof**;
its historical fixture and RPC proof must not be reported as fresh evidence.

The immutable runtime root is:

```text
/Users/kennethjiang/.codex/worktrees/hermes-post-stonks-campaign/hermes-feed/.runtime/post-stonks-38fc684/campaign-20260717T120432Z
```

The companion
`POST_STONKS_FRESH_PAPER_CAMPAIGN_2026-07-17_38FC684.json` is the
machine-readable authority for every value below, including every window
artifact hash and byte count.

## Safety boundary

Only Robinhood's public read-only RPC and direct WSS endpoints were used. No
wallet, private key, keystore, signer, broadcast path, execution path, canary,
deployment, Droplet, or server was accessed. Every finalized plan retained
`execution_eligible=false` and `broadcast=false`; every readiness row retained
`authorizes_canary=false` and `execution_eligible=false`.

## Exact tuple and orchestration

The operator recorded the worktree as clean at source SHA `38fc684`, on branch
`codex/post-stonks-fresh-campaign`, with parent
`e5ca09dc6c008cfd0decbe6ddbefba762bd67e26`, and recorded a fresh isolated
release build before acquisition. Those are checkout/build observations, not
fields bound by the immutable runtime root. The runtime campaign lock and
manifests bind the seven executable bytes, expected pins, pin-snapshot binary,
evidence-report binary, and local FIFO runner. They do not bind the source SHA
or the top-level `run-launchpad-paper-campaign.sh` bytes.

| Component | SHA-256 | Keccak-256 |
| --- | --- | --- |
| `hermes-feed` | `df7c2d5705d1ad405fa218d14f770b2c6a6c73bcaf394d43fb47a3c80298d6bb` | `0x2871ad950426a5818033697af4c8698d0182594224bb37913afe7a7f3c5ac301` |
| `hermes-launchpad-paper` | `46aa63d169537efe03204f349ed6c5dc847dc2b04fb88cea4df9c00225c46ffb` | `0x6b1f240acd37f1765ba823886e3b2aa87e458d27c4b20ab39901dd6fc6fe621f` |
| `hermes-launchpad-reconcile` | `1aea23e228718271c2bd6c09d286e926f66385753649165596183090d7c3b01b` | `0xd984d00ef76f85d54265cd5a323bda893e3d1632a57de41b90b17dde014892d4` |
| `hermes-launchpad-chain-head` | `0c274fd0cdf30dbb99a8ee8499cee68c51617a01c253e0236a9164a88f234797` | `0x7d1a6df1286b08bf72a2cfe575d0536089076055c27d921ec13cb7ffaf265a38` |
| `hermes-launchpad-readiness` | `5832da5200364f9e26bfbd64aab8fdae5e9c7d1cc1125450d375c08c32b856dc` | `0x79b76be11d407875ed59e0ac11f824eec7af41492d91df8e25b5f754613fe541` |
| `hermes-launchpad-pin-snapshot` | `9b9ed6cacb5dc8a44a56fd239d235049bc4f1025b5a5af5e1cddb649eea6b525` | `0xd9105e290e12832b9db07c8fedff0044f1905d2ef8e803cf2605c4c609628ddd` |
| `hermes-launchpad-evidence-report` | `0ff3b7be0f44edd0a30997a99c57dd29075d86be88563d8757d9628428d9e5f4` | `0xd9c443cd72e466a0947bc04b1438606f252f8c494fe9f339e37406f3a13e388a` |
| Campaign runner (supplemental checkout hash; not runtime-bound) | `4b6bae153ee747164a99ffee3f51ac323344c8ac06700a627e90b7e2ed07edf6` | `0x11c708fbb10e0f9d77659517762d035c4e1d32d271e5fde8ed275b582b24a7c1` |
| Local FIFO runner | `372770284a12a7f43b68208fc3bd244e0fdf82db1fed2f1e0f177027faf8aa45` | `0xb065b10aa70b8caaa586298346ac06d11f27871dd6c35e0e6f3249e876356fa3` |

The operator also recorded both runner fixture suites passing. That
supplemental checkout result covers the private FIFO/tee topology, phase
publication, manifest-last behavior, semantic replay and byte-identical
finalization, pin drift, tuple replacement, interruption, stale/overlapping
windows, partial sessions, and artifact tampering. It is not retroactively
represented as an original campaign-manifest field.

## Expected pins and fresh snapshots

The reviewed expected-pin file was frozen without modification: SHA-256
`36e632865b3fef00217fe5cb454f1fb7272025e81a8d25c3369eee0d4c1b8820`,
content Keccak-256
`0x3a192547af7a76a47d5de1b8102da0dc0bb77b7047a53c855b7d264aefa1a34c`.
No observed value was copied into expected configuration.

| Window | Fresh snapshot L2 / hash | L1 / timestamp | Snapshot SHA-256 | Snapshot report SHA-256 | RPC |
| --- | --- | --- | --- | --- | --- |
| A | `12104929` / `0x8da3670e21b33e6ac6c07648778f1f2113acbeddd6b5a13531ee921561f13e4c` | `25552288` / `1784289871` | `bb407c8a816f6a5610afa1aa8e6c5f30a768a39aa937477847df406d98a3949e` | `87dd848eb204e59b946cef8e652c2d392c5fccc0fbcdf51b5e72ad75398f3c72` | 77 attempts, 0 errors/retries |
| B | `12108508` / `0xfa1f7465bae98e03c93e4feae178bbfec7e417b81c77dd61c408c6a51cedcf7e` | `25552318` / `1784290231` | `39ad877ed8544d8575490c5a09a9599d89bb808989702e3b7a192bcecd8625e5` | `472e5d23efddf1cd2d9c4f4b4b66f19584a90c2f45de422293192465606981cc` | 77 attempts, 0 errors/retries |
| C | `12112408` / `0x82ee1bb04350a538775242c1e54f28e87f63cce91eaaac762a4397a301151887` | `25552350` / `1784290622` | `c5af2c622e8f3a64d7c030b2f69dd50d97f19e53d5175578ea2123335350d10a` | `3c487c2dd4d0218febb8fad2a58869f78890438843b2d530bfd7c1e908b50a68` | 77 attempts, 0 errors/retries |

All three snapshot reports state `expected_validation_passed=true` and
`pin_count=41`.

## Correct pipeline and window integrity

Each window used the required split-output data path:

```text
hermes-feed probe --record private-mode-0600-raw-FIFO
  -> tee raw-feed.jsonl -> private-mode-0600-observer-FIFO
  -> hermes-launchpad-paper --acquisition live --input -
```

Probe stdout contained metrics only. After producer close and FIFO drain, the
independent collector scanned the anchored interval, reconciled receipts and
events, and the paper finalizer generated plans. The completion manifest was
published last.

| Window | Scored range | Start hash | Cutoff hash | Manifest SHA-256 | Manifest Keccak-256 |
| --- | --- | --- | --- | --- | --- |
| A | `12105003..12107982` | `0x30b7812ec691061ad090497c94d9daf78cf3000074ad1447fa4b317207ab1520` | `0x2671ac31c4d428e7621aa47789b2133e00733f8958b05bba978e526d208a1af0` | `7935904005af565d52ce543ac57604a5cb7e47b41b8e3c6fc4219b6a40a098a1` | `0xbb86e783955ed63906d835a711c149150b73e00a8619579c2f1e8736c21b0d0f` |
| B | `12108577..12111560` | `0x36ba08f8235b11fd47cf11a08d157e88ab3bfa42966b10564bc9dd7344229c35` | `0x3164dad8dd709d585178c719989d67a69a1eee44d78f8c6f86b88abae565da1c` | `e0e47e24df5b82de6f85afd213f56a691798131bedf1d07f77e12bccfb1edb07` | `0xe4fda75b8d525da9771909cdb61973ad32f4f772e5581650901e26e903b1f081` |
| C | `12112481..12115460` | `0x61cd6ef54527819c141312b4d11a4dc13672c55fbbc607c0a4e8aa5f15ab80ae` | `0xb868671e8d6806becca5770db8836aedf12ff3d8840c8e7e09f348f1999448d2` | `d286ea2229349b836b598109cac852699d9669684e2d08910d9e7d58ca8705de` | `0x0208091d3d6b42d333f161e8e5d1f7caea6e3b8700c742cf2d0a30625fff498b` |

The ranges are strictly non-overlapping. Every continuity sequence is exactly
`connected, coverage_closed`. The completed root contains no `.partial` file,
FIFO, or symlink. Every file is mode `0600` and every directory mode `0700`.

### Manifest-bound artifact hashes

Each manifest binds nine canonical artifacts. Values below are Keccak-256;
byte counts and the same mapping are retained in the companion JSON.

| Artifact | A | B | C |
| --- | --- | --- | --- |
| `cutoff-anchor.txt` | `0x008a961c769242d7a250f11617b3375b6acc416c0688845468a1a70f58da2234` | `0xff3f7f4403fdb386d67fb1f18a6c22ced451f0a47bbc5da727e32e85845fe6e6` | `0x45ddff80d0c162c9cddda77e3ae1c2a2d6253e2d14cb72a752c424f0f153111b` |
| `expected-pins.input.json` | `0x3a192547af7a76a47d5de1b8102da0dc0bb77b7047a53c855b7d264aefa1a34c` | same | same |
| `launchpad-paper-finalized.jsonl` | `0xf20b596e6619b117c5c17003c0e887fbcf0c8f910dc01a197537663a1f3125df` | `0x3dbc982e160130d55f8044a526bb310ca7c024504f0e9317253cb68fd1f90fa7` | `0x51f84e144139c175ce35a58c225f73acfc2255df6ef8c6d597b0c7042f5e40d7` |
| `launchpad-paper.jsonl` | `0xd9ff8ac10017804b2f77f7d53c25a7e5f39c5a7a55e1733c93051f09df179f61` | `0x4d38434f2c3178bbe3ad552ec155a3e4d46f03066564ff7ef7d6c63fe1c47fbd` | `0x85a70d52413be07d4727e714e7eacb1b3ba992863c020d949c48dffce8ebbcc7` |
| `observed-startup-snapshot.input.json` | `0xdd72d0b0de76feaf2aa027c04776f7ffefc23760b4d04f0ce6a8c3a1a3179a9b` | `0x544397672a8ad1b703e65283beeab93d8e965e5ae30a824b4f17b0e97d3ec20b` | `0x3ced615e8cbda31c10c44a691e454034f4f5237c941ef61184b1c2b38991a58d` |
| `probe-metrics.jsonl` | `0x339a734e5f9b3a77149caaf2c924ff77d3e2ccde1423b8f9833861ad34f22235` | `0xb2a99cdd99cefddec59fa532ce7d4252a4d1b88c9f8ac2778a341195773fd190` | `0xcfd2a1bf24afca3c81d52b4acf01b9cb856bcb81e7a8dea9c5a51897b0a7f844` |
| `raw-feed.jsonl` | `0xd8a73d0b3f9a6b9d18f07449c0aa4354a766ec4f4ab70fb9cbcab6eb2da67cec` | `0xb0f6cae9d8b6530f069f8da76c4ce3bb8db647ae1f94e93e7d86e8c0989b8290` | `0x4c6a600eaa0c0b1001c6155903294bb68ed21027c519d2d79d3fe7fd30fc0be5` |
| `reconciliation-evidence.jsonl` | `0x4f9082ffa4d8660ab68a6d5af0c8d54298718efd7f5f626ed68f196ec2edd728` | `0xc5c47edc6063c8f296cfe1d00781426d1bf654bc297abb083d5ffcc3abb32e87` | `0x6b4f2e48ba9c76996013db114f91c0f60a9b71d44ca45f7d72d6f723de94c216` |
| `start-anchor.txt` | `0x69ca76d1265c408c0abf033b1cf021824882a98447c3cfa3b29e092791d16783` | `0xa19bca766311721ea9960ddf99274c410c8f4e62289b90d8b51052a04fdaafcc` | `0xee69959ae5a3b448a70f2f07de4f6faa917bc99571cf40db2cb46c0fbb3ce7c9` |

The authoritative readiness SHA-256 is
`5931a1c63f35ed3805932ddaf76bc805e31696f0be699edc6a7371534a4d3c74`;
the evidence-report SHA-256 is
`2a0b61a83eb3bdd44a3a6cac299571593b00e803b018211e3178d3126bfd7d3d`.
The pinned binaries regenerated both byte-for-byte.

## Results

Latency values are observer p50/p95/p99 in microseconds. `Confirmed/eligible`
means confirmed observations and quote-eligible confirmed observations.

| Launchpad | Window A | Window B | Window C | Aggregate confirmed/eligible | Aggregate latency µs | FP / misses / identity / direction / prediction / quote |
| --- | --- | --- | --- | --- | --- | --- |
| Bow | `0/0` | `0/0` | `0/0` | `0/0` | n/a | `0/0/0/0/0/0` |
| LaunchHood V3 | `4/4` | `2/2` | `2/2` | `8/8` | `273/555/555` | `0/0/0/0/0/0` |
| Clanker | `19/19` | `39/39` | `0/0` | `58/58` | `380/945/1503` | `0/0/0/0/0/0` |
| Bankr/Doppler | `10/10` plus 1 miss | `8/8` plus 1 miss | `1/1` | `19/19` plus 2 misses | `423/1478/1478` | `0/2/0/0/0/0` |
| Pons | `21/0` | `29/0` plus 1 miss | `22/0` | `72/0` plus 1 miss | `401/1286/1477` | `0/1/0/0/0/0` |
| Hood | `0/0` | `0/0` | `0/0` | `0/0` | n/a | `0/0/0/0/0/0` |

There were zero feed-coverage misses and zero unreconciled observations. The
confirmed Bankr profile sample is CurveTicksV4 `19`, split ERC-7579 `17` and
direct Airlock `2`. The confirmed Clanker sample is pinned-extension
five-position `58`; extensionless single-position remains `0`. LaunchHood's
embedded-buy profile has `8`. Pons's 72 confirmed observations were legacy and
therefore quote-ineligible; the one fresh current-generation truth was missed.

### Entry, slippage, and immediate exit simulation

Every admitted plan independently sized entry at `0.001 WETH`
(`1000000000000000` wei) with `100` bps slippage. Entry and immediate
full-position exit counts are equal.

| Launchpad | Entry/exit plans | Expected round-trip return | Immediate simulated outcome |
| --- | --- | --- | --- |
| LaunchHood V3 | `8/8` | `9801` bps | about `-199` bps before applying the exit min-receive bound |
| Clanker | `58/58` | `849` bps | about `-9151` bps; the receipt-end immediate exit is highly adverse |
| Bankr/Doppler | `19/19` | `97` bps | about `-9903` bps; the receipt-end immediate exit is highly adverse |
| Bow / Pons / Hood / Stonks | `0/0` | n/a | no fresh quote outcome |

These are static receipt-end paper simulations, not forecasts and not trading
authorization. The very adverse Clanker and Bankr immediate exits are evidence
against promotion, not an invitation to tune away the result.

## Stonks V3 and shared-Airlock suppression

Across all three windows there were zero exact Stonks ground-truth
transactions, zero Stonks reconciliation rows, zero Stonks quote rows, and
zero finalized Stonks plans. Consequently, the observed shared-Airlock
conflict count is zero only **vacuously**: the suppression branch was not
exercised by fresh traffic.

The checked historical Stonks fixtures retained their expected SHA-256 values:

- paper quote: `5a01f176480c6e8e617de9e3c4eb2a21a7e5c33379287a552fb3d3ed2d02de52`;
- first-swap differential: `979175df35050b22918447779a15e08c7eacfd4cbe03fdd7573c8fe4a34799ed`;
- concrete RPC transcript: `c23ebcd4b1af5c474b6ea9c999cfd766ab7753ae740c255f3f2bf77f06733819`.

Those hashes validate historical fixtures only. They are not fresh campaign
observations and do not clear a Stonks sample or readiness requirement.

## Independent miss forensics

The supplemental
`POST_STONKS_FRESH_PAPER_CAMPAIGN_2026-07-17_38FC684_MISS_FORENSICS.json`
makes the three forensics results independently reproducible. Its SHA-256 is
`9baef8a9abed896128090ea7d4188fb1fca1f184ee8b528524b40c46a40929ea`,
its Keccak-256 is
`0x011029a7d8989a691ef3428315083450b88a8f5f183251b047cb183e06c02b7f`,
and it is 19,109 bytes. For every miss it binds the exact campaign
reconciliation row; public read-only `eth_getTransactionByHash`,
`eth_getTransactionReceipt`, `eth_getBlockByHash`, and receipt-block
`eth_getCode` request parameters; canonical sorted-result digests; raw calldata
digests; account designator and delegated runtime proofs; and deterministic ABI
decode signatures and results. An independent reviewer can repeat the recorded
requests, canonicalize `.result` with `jq -cS`, compare both digests, and repeat
the listed `cast` decodes.

This is supplemental evidence collected after the campaign. It did not mutate
the campaign windows and is not part of the original session manifests. It
closes the reproducibility gap without retroactively claiming a stronger
original provenance boundary.

### Bankr reverse-orientation CurveTicksV4: two misses

The two misses are:

| Tx | L2 | UserOperation leader | Token | Pool ID |
| --- | --- | --- | --- | --- |
| `0xccf350bfed931d136f9fbf5bc20fe49eb1404a5912aae2684549e32be68e4567` | `12107164` | `0x9B60922370Fe0F6bd079E0302612bfB9a3045c3d` | `0x0106e926a9ccaedce5f87c859beaf89a56e96ba3` | `0x1d40b4a7fb7768a884ea86f285e00cb9fd5ca3d282680984875888c6e5b81720` |
| `0x88eaff5b44775c8ccda07bae4ca7548cf448093013d6c7326fac3c0c8d4b2c49` | `12109627` | `0x289A2b7758257A666C7561bB6B680F26eF4BE208` | `0x022df6568187016fde9651cb8b5bc4aedcf80ba3` | `0xa3a17284bba4c29e85ace5fe502177d3c8db97a45b5069e2b1ba467418047832` |

Verified facts:

- Both are successful, single-operation EntryPoint v0.7 transactions with
  outer selector `0x765e827f`.
- Both accounts use `execute(bytes32,bytes)` selector `0xe9ae5c53`, all-zero
  mode, zero call value, Airlock target
  `0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862`, and inner selector
  `0x882db707`.
- At each receipt block, the leader code is the exact 23-byte EIP-7702
  designator for Kernel `0xd6cedde84be40893d153be9d467cd6ad37875b28`, hash
  `0x4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4`.
  No factory is present; the required delegation implementation is complete.
- The reviewed Bankr token/governance/initializer/migrator/hook/integrator
  addresses match. The create calls are exact CurveTicksV4:
  `[-229400,-119200]` at 99% and `[-119200,887200]` at 1%, fee `7000`, spacing
  `200`, far tick `887000`.
- Both receipts initialize `currency0=token`, `currency1=WETH`; both token
  addresses are numerically below WETH.

The rotating per-account runtime/delegation proof therefore passed. The exact
downstream blocker is the unsupported reverse CurveTicksV4 token/WETH
orientation. Production currently admits V4 only when token is above WETH;
the generic emitted error was
`bankr_strict_quote:receipt launch identity is missing, duplicated, or inconsistent`.

Inference: these are the same reviewed Bankr V4 semantics in the opposite
canonical pool orientation, not Stonks and not a new account ABI. Unknown until
implemented and independently reviewed: the complete reverse-orientation
receipt range ordering, quote direction, rounding, and negative boundaries.

Bounded next action: add a paper-only reverse-orientation V4 proof fixture from
both transactions, derive exact mirrored receipt ranges and state transitions,
add negative orientation/range tests, and independently review it before
another immutable campaign. Do not widen account selectors or execution
authority.

### Pons current-generation rotating self-batch: one miss

Transaction
`0x8fa866828e07b65be51bf88ee429c9ce9cdbda06e900585b7dee90a551a92165`
at L2 `12110485` is a successful current-generation Pons launch. It created
token `0xa817ac2c627004ba83dd5cf24762b119d89d2869` and pool
`0x50086639b2a00a5adc87453eeaccece5ddab069d`.

Verified facts:

- It is type 4 on chain 4663, with signer/self-target
  `0x50f70cf40c32826095a2a350d6ac523bddf0ef82`, outer value zero, and selector
  `0x3f707e6b`.
- Its singleton authorization delegates to the same pinned implementation
  `0xdc44136e7ca3509a73fc6c22b6a6bd302bf9a1e2`; the receipt-block designator
  and implementation runtime hashes exactly match the reviewed hashes
  `0x9bdfa4...3f3` and `0x6d7379...baa`.
- The two ordered calls use the same auxiliary target and exact auxiliary
  value `538332961881668`, followed by current Pons factory
  `0x0c37a24f5d23a486fa692d1500881d698b1f77a4` and selector `0x686399cb`.
  Both config IDs are zero and developer wallet equals the signer.
- The inner value is `30500000000000000` wei: the fixed
  `500000000000000` launch fee plus the receipt's
  `30000000000000000` initial buy.

It is not the one reviewed wrapper proof profile. The account differs from
`0xfb3538b3fac2cc5ffc582446c55875a889abd146`, authorization nonce is `27`
rather than `15`, inner value/initial buy are `30.5e15/30e15` rather than
`60.5e15/60e15`, and transaction hash/block/index differ from the pinned proof.
The observer correctly rejected it. The reconciler independently found the
current Pons event, then the non-normalized quote path failed the transaction
envelope with
`pons_quote_error:transaction, receipt, or paper policy envelope is invalid`.

Inference: the profile appears to be a rotating-account/current-Pons sibling
of the historical proof, but one fresh transaction is not sufficient to define
generic authorization nonce or sizing policy. Unknown: whether arbitrary
accounts/nonces and varying initial buys are stable protocol semantics, and
which authorization implementation versions may appear.

Bounded next action: keep this profile observe-only, collect additional exact
type-4 current-Pons examples, independently prove the rotating authority and
delegation boundary, and derive a bounded paper-only normalization policy with
negative account/authorization/call-order/value tests. Do not generalize global
`0x3f707e6b` dispatch.

## Readiness

The campaign accepted three windows but no launchpad met the full thresholds.
LaunchHood has only 8 of 100 required quote-eligible confirmations and fewer
than 10 embedded-buy profile observations. Clanker has 58 of 100 and zero
extensionless observations. Bankr has 19 of 100, incomplete historical profile
coverage, only two direct-Airlock observations, and two detector misses. Pons
has zero current-generation quote-eligible observations and one detector miss.
Bow and Hood have no sample. Stonks has no fresh transaction at all.

No promotion, canary, execution, broadcast, or server work is supported by
this evidence.
